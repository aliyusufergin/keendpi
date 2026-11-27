//! `gecit check`: for each domain, is it blocked, and does the bypass get it through?
//!
//! Both probes run from the router itself. Each opens a TCP connection to port 443 and sends a
//! ClientHello for the domain; any reply from the server (ServerHello or alert) means it got
//! through, while a reset, a close or silence means it was blocked.
//! - direct: the socket carries MARK, so the rules skip it and the engine never sees it;
//! - via gecit: the socket carries FORCE_MARK, so the engine sends its fake whatever the lists say.
//! TLS only: plain-HTTP blocks are not probed. A plain-HTTP answer counts as a block page (Kablonet's
//! "Güvenli İnternet" filter answers so); any other reply counts as the server.

use crate::config::Config;
use crate::engine::Lists;
use crate::supervise::queue_stats;
use crate::{rawsock, tls};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::fd::FromRawFd;
use std::time::Duration;

const CONTROL: &str = "example.com";
const TIMEOUT: Duration = Duration::from_secs(4);
const PARALLEL: usize = 8;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Outcome {
    Ok,
    Reset,
    Closed,
    Timeout,
    Redirect,
    NoConnect,
}

impl Outcome {
    fn text(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Reset => "blocked (reset)",
            Outcome::Closed => "blocked (closed)",
            Outcome::Timeout => "blocked (no answer)",
            Outcome::Redirect => "blocked (block page)",
            Outcome::NoConnect => "no connection",
        }
    }
}

fn connect_marked(addr: Ipv4Addr, mark: u32) -> std::io::Result<TcpStream> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let s = TcpStream::from_raw_fd(fd); // closes fd on every path from here
        let m = mark;
        if libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_MARK, &m as *const u32 as *const libc::c_void, 4) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let sa = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: 443u16.to_be(),
            sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(addr.octets()) },
            sin_zero: [0; 8],
        };
        let r = libc::connect(
            fd,
            &sa as *const libc::sockaddr_in as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        if r < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(std::io::Error::last_os_error());
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
        if libc::poll(&mut pfd, 1, TIMEOUT.as_millis() as libc::c_int) <= 0 {
            return Err(ErrorKind::TimedOut.into());
        }
        if let Some(e) = s.take_error()? {
            return Err(e);
        }
        s.set_nonblocking(false)?;
        Ok(s)
    }
}

fn probe(addr: Ipv4Addr, name: &str, mark: u32) -> Outcome {
    let Ok(mut s) = connect_marked(addr, mark) else { return Outcome::NoConnect };
    let _ = s.set_read_timeout(Some(TIMEOUT));
    let _ = s.set_write_timeout(Some(TIMEOUT));
    let hello = tls::fake_client_hello(name.as_bytes(), 517, &mut rawsock::fill_random);
    if s.write_all(&hello).is_err() {
        return Outcome::Reset;
    }
    let mut buf = [0u8; 64];
    match s.read(&mut buf) {
        Ok(0) => Outcome::Closed,
        // A TLS record (ServerHello or alert) is the server; plain HTTP is a filter's block page.
        Ok(_) if buf.starts_with(b"HTTP/") => Outcome::Redirect,
        Ok(_) => Outcome::Ok,
        Err(e) if e.kind() == ErrorKind::ConnectionReset => Outcome::Reset,
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Outcome::Timeout,
        Err(_) => Outcome::Reset,
    }
}

fn resolve(name: &str) -> Option<Ipv4Addr> {
    (name, 443).to_socket_addrs().ok()?.find_map(|a| match a {
        SocketAddr::V4(v) => Some(*v.ip()),
        SocketAddr::V6(_) => None,
    })
}

struct Row {
    name: String,
    note: &'static str,
    addr: Option<Ipv4Addr>,
    direct: Option<Outcome>,
    via: Option<Outcome>,
}

pub fn run(cfg: &Config, domains: Vec<String>) -> Result<(), String> {
    let lists = Lists::load(cfg)?;
    let running = queue_stats(cfg.queue).is_some();
    let mut names: Vec<(String, &'static str)> = if domains.is_empty() {
        let mut v: Vec<(String, &str)> = lists.include_names().into_iter().map(|n| (n, "")).collect();
        v.extend(lists.learned_names().into_iter().map(|n| (n, "learned")));
        v
    } else {
        domains.into_iter().map(|n| (n, "")).collect()
    };
    names.push((CONTROL.into(), "control"));
    // The forced probe ignores the lists; say when the engine would leave a name alone.
    for (n, note) in names.iter_mut() {
        if note.is_empty() && !lists.wants(cfg.mode, n.as_bytes()) {
            *note = if lists.wants(crate::config::Mode::All, n.as_bytes()) { "not listed" } else { "excluded" };
        }
    }

    println!(
        "gecit {}: mode {:?}, {}; engine {}",
        env!("CARGO_PKG_VERSION"),
        cfg.mode,
        lists.summary(),
        if running { "running" } else { "NOT running" }
    );
    // A few at a time: enough to finish quickly, few enough not to flood the engine's queue.
    let mut rows: Vec<Row> = vec![];
    for chunk in names.chunks(PARALLEL) {
        std::thread::scope(|sc| {
            let hs: Vec<_> = chunk
                .iter()
                .map(|(name, note)| {
                    sc.spawn(move || {
                        let addr = resolve(name);
                        let direct = addr.map(|a| probe(a, name, cfg.mark));
                        let via = addr.filter(|_| running).map(|a| probe(a, name, cfg.force_mark));
                        Row { name: name.clone(), note, addr, direct, via }
                    })
                })
                .collect();
            rows.extend(hs.into_iter().filter_map(|h| h.join().ok()));
        });
    }

    let w = rows.iter().map(|r| r.name.len() + if r.note.is_empty() { 0 } else { r.note.len() + 3 }).max().unwrap_or(10).max(6);
    println!("{:w$}  {:15}  {:20}  {}", "domain", "address", "direct", "with gecit");
    for r in &rows {
        let label = if r.note.is_empty() { r.name.clone() } else { format!("{} ({})", r.name, r.note) };
        let addr = r.addr.map(|a| a.to_string()).unwrap_or_else(|| "no DNS answer".into());
        let direct = r.direct.map(|o| o.text()).unwrap_or("-");
        let via = match (r.addr, r.via) {
            (None, _) => "-",
            (_, Some(o)) => o.text(),
            (_, None) => "engine not running",
        };
        println!("{label:w$}  {addr:15}  {direct:20}  {via}");
    }
    if rows.iter().any(|r| r.direct == Some(Outcome::NoConnect) && r.via != Some(Outcome::Ok)) {
        println!(
            "\n\"no connection\" usually means the DNS answer is the ISP's block address, not the site.\n\
             gecit does not fix DNS: use DNS-over-HTTPS/TLS on the router and remove plain DNS servers."
        );
    }
    Ok(())
}
