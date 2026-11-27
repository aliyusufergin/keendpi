//! The worker: reads queued packets, sends fakes ahead of blocked requests, learns blocked domains.
//!
//! Packets it receives (see the rules in files/rules.sh):
//! - outgoing first segments of TLS ClientHellos and HTTP requests (mangle POSTROUTING);
//! - incoming SYN-ACKs from those ports, to measure the server's hop distance;
//! - incoming RSTs from those ports, and plain-HTTP answers on port 443: the block signals that
//!   `auto` mode learns from.
//!
//! Every packet is accepted unchanged; problems only mean "no fake for this one".

use crate::config::{Config, Mode};
use crate::hostlist::{self, HostList};
use crate::packet::{self, ACK, RST, SYN, Tcp4};
use crate::{http, log, rawsock, tls};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const NF_INET_FORWARD: u8 = 2;
/// A reset this soon after the request counts as a block signal.
const BLOCK_WINDOW: Duration = Duration::from_secs(3);
const FLOW_TTL: Duration = Duration::from_secs(20);
const MAX_FLOWS: usize = 16384;
const MAX_LEARNED: usize = 2000;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Key {
    client: Ipv4Addr,
    cport: u16,
    server: Ipv4Addr,
    sport: u16,
}

struct Flow {
    seen: Instant,
    /// Hops to the server (the server itself counts as one), from its SYN-ACK.
    hops: Option<u8>,
    /// When the client's SYN left, and the round trip to the server (SYN to SYN-ACK).
    syn_at: Option<Instant>,
    rtt: Option<Duration>,
    /// The latest request on this flow: when, the name it asked for, whether we sent a fake.
    request: Option<(Instant, Option<String>, bool)>,
}

pub struct Lists {
    include: HostList,
    auto: HostList,
    exclude: HostList,
}

impl Lists {
    fn all_paths(cfg: &Config) -> Vec<std::path::PathBuf> {
        let mut v = cfg.lists.clone();
        v.extend(cfg.exclude.iter().cloned());
        v.push(cfg.auto_list.clone());
        v
    }

    pub fn load(cfg: &Config) -> Result<Lists, String> {
        Ok(Lists {
            include: hostlist::load(&cfg.lists)?,
            auto: hostlist::load(std::slice::from_ref(&cfg.auto_list))?,
            exclude: hostlist::load(&cfg.exclude)?,
        })
    }

    pub fn summary(&self) -> String {
        format!("{} listed, {} learned, {} excluded", self.include.len(), self.auto.len(), self.exclude.len())
    }

    pub fn include_names(&self) -> Vec<String> {
        self.include.names()
    }

    pub fn learned_names(&self) -> Vec<String> {
        self.auto.names()
    }

    /// Whether a connection to `name` gets the bypass in `mode`. The exclude list always wins.
    pub fn wants(&self, mode: Mode, name: &[u8]) -> bool {
        if self.exclude.matches(name) {
            return false;
        }
        match mode {
            Mode::All => true,
            Mode::Auto => self.include.matches(name) || self.auto.matches(name),
            Mode::List => self.include.matches(name),
        }
    }
}

/// A name that is safe to write into a list file: a DNS name with at least two labels.
fn learnable(name: &[u8]) -> bool {
    name.len() <= 253
        && name.contains(&b'.')
        && !name.starts_with(b".")
        && !name.ends_with(b".")
        && !name.windows(2).any(|w| w == b"..")
        && name.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
        && !name.iter().all(|&c| c.is_ascii_digit() || c == b'.')
}

/// Hops to the sender of a packet that arrived with `ttl`, assuming it started at 64, 128 or 255.
/// In FORWARD the router has already decremented the TTL once.
fn distance(ttl: u8, hook: u8) -> u8 {
    let init: u16 = match ttl {
        0..=64 => 64,
        65..=128 => 128,
        _ => 255,
    };
    let d = init - ttl as u16 + if hook == NF_INET_FORWARD { 0 } else { 1 };
    d.min(255) as u8
}

/// All disk access (list reloads, learned-domain appends) happens on this thread, so a slow or
/// hung USB disk never holds up packets.
fn disk_thread(cfg: Config, learned: Receiver<String>, lists: Sender<Lists>) {
    let paths = Lists::all_paths(&cfg);
    let mut stamp = hostlist::stamp(&paths);
    // What the auto list file holds, so a name is never appended twice.
    let mut in_file = hostlist::load(std::slice::from_ref(&cfg.auto_list)).unwrap_or_default();
    loop {
        match learned.recv_timeout(Duration::from_secs(5)) {
            Ok(name) if in_file.matches(name.as_bytes()) => {}
            Ok(name) => {
                in_file.insert(name.as_bytes());
                if let Err(e) = hostlist::append(&cfg.auto_list, &name) {
                    log!("gecit: {}: {e}", cfg.auto_list.display());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let now = hostlist::stamp(&paths);
        if now != stamp {
            stamp = now;
            match Lists::load(&cfg) {
                Ok(l) => {
                    in_file = l.auto.clone();
                    log!("gecit: lists reloaded: {}", l.summary());
                    if lists.send(l).is_err() {
                        return;
                    }
                }
                // Keep the old lists; try again when the files change again.
                Err(e) => log!("gecit: lists not reloaded: {e}"),
            }
        }
    }
}

/// A reset `delay` after the request, on a path with round trip `rtt`, beat the server: no reply
/// from it can come sooner than one round trip. Half leaves room for jitter. Servers closer than
/// about twice the DPI's distance (~3 ms on Kablonet) can't be told apart and are never learned.
fn injected(delay: Duration, rtt: Duration) -> bool {
    delay * 2 < rtt
}

pub struct Engine {
    cfg: Config,
    verbose: bool,
    lists: Lists,
    flows: HashMap<Key, Flow>,
    raw: rawsock::RawSock,
    last_prune: Instant,
    learned_tx: Sender<String>,
    lists_rx: Receiver<Lists>,
    send_errors: u64,
    resets_after_fake: u64,
    learn_full_logged: bool,
}

impl Engine {
    pub fn new(cfg: Config, verbose: bool) -> Result<Engine, String> {
        let lists = Lists::load(&cfg)?;
        let raw = rawsock::RawSock::new(cfg.mark).map_err(|e| format!("raw socket: {e}"))?;
        let (learned_tx, learned_rx) = mpsc::channel();
        let (lists_tx, lists_rx) = mpsc::channel();
        let c = cfg.clone();
        std::thread::Builder::new()
            .name("disk".into())
            .spawn(move || disk_thread(c, learned_rx, lists_tx))
            .map_err(|e| format!("disk thread: {e}"))?;
        Ok(Engine {
            cfg,
            verbose,
            lists,
            flows: HashMap::new(),
            raw,
            last_prune: Instant::now(),
            learned_tx,
            lists_rx,
            send_errors: 0,
            resets_after_fake: 0,
            learn_full_logged: false,
        })
    }

    pub fn run(mut self) -> std::io::Result<()> {
        let q = self.cfg.queue;
        let mut nfq = nfq::Queue::open()?;
        nfq.bind(q)?;
        nfq.set_fail_open(q, true)?;
        // Short queue: if the worker hangs, the kernel accepts packets once it is full, and the
        // supervisor restarts the worker well before that matters.
        nfq.set_queue_max_len(q, 64)?;
        nfq.set_copy_range(q, 4096)?;
        log!(
            "gecit: worker on queue {q}, mode {:?}, ttl {}, {}",
            self.cfg.mode,
            self.cfg.ttl,
            self.lists.summary()
        );
        loop {
            let mut msg = match nfq.recv() {
                Ok(m) => m,
                // The kernel dropped messages for us (fail-open accepted them), or a signal.
                Err(e) if matches!(e.raw_os_error(), Some(libc::ENOBUFS) | Some(libc::EINTR)) => continue,
                Err(e) => return Err(e),
            };
            let now = Instant::now();
            self.handle(msg.get_payload(), msg.get_nfmark(), msg.get_hook(), now);
            msg.set_verdict(nfq::Verdict::Accept);
            if let Err(e) = nfq.verdict(msg) {
                log!("gecit: verdict: {e}");
            }
            self.housekeeping(now);
        }
    }

    fn handle(&mut self, pkt: &[u8], mark: u32, hook: u8, now: Instant) {
        let Some(t) = packet::parse_tcp4(pkt) else { return };
        let f = t.flags();
        if f & (SYN | ACK | RST) == SYN | ACK {
            let key = Key { client: t.dst(), cport: t.dport(), server: t.src(), sport: t.sport() };
            let hops = distance(t.ttl(), hook);
            if let Some(e) = self.flow(key, now) {
                e.hops = Some(hops);
                e.rtt = e.syn_at.map(|s| now.duration_since(s));
            }
        } else if f & (SYN | ACK | RST) == SYN {
            let key = Key { client: t.src(), cport: t.sport(), server: t.dst(), sport: t.dport() };
            if let Some(e) = self.flow(key, now) {
                e.syn_at = Some(now); // a retransmitted SYN restarts the measurement
            }
        } else if f & RST != 0 {
            let key = Key { client: t.dst(), cport: t.dport(), server: t.src(), sport: t.sport() };
            self.on_block_signal(key, now, false);
        } else if t.sport() == 443 && t.payload.starts_with(b"HTTP/") {
            // A plain-HTTP answer to a ClientHello: a filter (Kablonet's "Güvenli İnternet"
            // middlebox) redirecting to its block page. No TLS server sends that.
            let key = Key { client: t.dst(), cport: t.dport(), server: t.src(), sport: t.sport() };
            self.on_block_signal(key, now, true);
        } else if !t.is_syn_fin_rst() && !t.payload.is_empty() {
            self.on_request(&t, mark & self.cfg.force_mark != 0, now);
        }
    }

    /// The flow's entry, created if there is room. The table is bounded between prunes too.
    fn flow(&mut self, key: Key, now: Instant) -> Option<&mut Flow> {
        if self.flows.len() >= MAX_FLOWS && !self.flows.contains_key(&key) {
            return None;
        }
        let e = self.flows.entry(key).or_insert(Flow { seen: now, hops: None, syn_at: None, rtt: None, request: None });
        e.seen = now;
        Some(e)
    }

    fn on_request(&mut self, t: &Tcp4, forced: bool, now: Instant) {
        enum Req<'a> {
            Name(&'a [u8]),
            Incomplete,
            NoName,
        }
        let (req, is_tls) = match tls::parse_sni(t.payload) {
            tls::Sni::Found(n) => (Req::Name(n), true),
            tls::Sni::Incomplete => (Req::Incomplete, true),
            tls::Sni::Absent => (Req::NoName, true),
            tls::Sni::NotClientHello if self.cfg.http => match http::parse_host(t.payload) {
                http::Host::Found(n) => (Req::Name(n), false),
                http::Host::Incomplete => (Req::Incomplete, false),
                http::Host::None => return,
            },
            tls::Sni::NotClientHello => return,
        };
        let name = match req {
            Req::Name(n) => Some(n.strip_suffix(b".").unwrap_or(n)),
            _ => None,
        };
        let apply = forced
            || match req {
                Req::Name(n) => self.lists.wants(self.cfg.mode, n.strip_suffix(b".").unwrap_or(n)),
                // The name lies in a later segment we will not see, and the fake must go out before
                // this one: send it. It is harmless, and the TTL cap keeps it from the server.
                Req::Incomplete => true,
                // Nothing for the DPI to match on.
                Req::NoName => false,
            };
        let key = Key { client: t.src(), cport: t.sport(), server: t.dst(), sport: t.dport() };
        let hops = self.flow(key, now).and_then(|f| f.hops);
        let shown = || name.map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_else(|| "?".into());

        let mut faked = false;
        if apply {
            let ttl = match hops {
                Some(h) => self.cfg.ttl.min(h.saturating_sub(1)),
                None => self.cfg.ttl,
            };
            if ttl == 0 {
                if self.verbose {
                    log!("gecit: {} -> {} is {} hop(s) away, no room for a fake", shown(), t.dst(), hops.unwrap_or(0));
                }
            } else {
                let payload = if is_tls {
                    tls::fake_client_hello(self.cfg.fake_host.as_bytes(), 517, &mut rawsock::fill_random)
                } else {
                    http::fake_request(&self.cfg.fake_host)
                };
                let r = self.raw.send(&packet::build_fake(t, &payload, ttl), t.dst());
                faked = r.is_ok();
                if r.is_err() {
                    self.send_errors += 1;
                }
                if self.verbose || (r.is_err() && (self.send_errors <= 5 || self.send_errors % 1000 == 0)) {
                    log!(
                        "gecit: {} -> {}:{} fake ttl {} (hops {:?}{}): {:?}",
                        shown(),
                        t.dst(),
                        t.dport(),
                        ttl,
                        hops,
                        if forced { ", forced" } else { "" },
                        r
                    );
                }
            }
        } else if self.verbose {
            log!("gecit: {} -> {}:{} passed", shown(), t.dst(), t.dport());
        }
        if let Some(f) = self.flows.get_mut(&key) {
            f.request = Some((now, name.map(|n| String::from_utf8_lossy(n).into_owned()), faked));
        }
    }

    /// A reset (`redirect` false) or an injected HTTP answer on a TLS port (`redirect` true).
    fn on_block_signal(&mut self, key: Key, now: Instant, redirect: bool) {
        let Some(flow) = self.flows.remove(&key) else { return };
        let Some((at, name, faked)) = flow.request else { return };
        if now.duration_since(at) > BLOCK_WINDOW {
            return;
        }
        let Some(name) = name else { return };
        if faked {
            self.resets_after_fake += 1;
            if self.verbose || self.resets_after_fake <= 5 || self.resets_after_fake % 100 == 0 {
                let what = if redirect { "redirected" } else { "reset" };
                log!("gecit: {name} {what} despite the fake ({} so far)", self.resets_after_fake);
            }
            return;
        }
        // Servers reset connections too, but no reply from the server can arrive sooner than one
        // round trip after the request. A reset well inside that came from a box on the way.
        // (Kablonet's DPI answers in ~1.6 ms against a ~9 ms round trip, and copies the server's TTL.)
        let delay = now.duration_since(at);
        let injected = redirect || flow.rtt.is_some_and(|rtt| injected(delay, rtt));
        if self.verbose && redirect {
            log!("gecit: {name} redirected to a block page");
        } else if self.verbose {
            // Short: the Keenetic log view cuts lines at ~80 characters.
            let rtt = flow.rtt.map_or("?".into(), |r| format!("{:.1}", r.as_secs_f64() * 1e3));
            let what = if injected { "block" } else { "not a block" };
            log!("gecit: reset {name} after {:.1} ms, rtt {rtt} ms: {what}", delay.as_secs_f64() * 1e3);
        }
        if !injected {
            return;
        }
        // Already covered (the fake could not be sent, e.g. too few hops): nothing to learn.
        if self.cfg.mode != Mode::Auto || !learnable(name.as_bytes()) || !self.lists.wants(Mode::All, name.as_bytes())
            || self.lists.wants(Mode::Auto, name.as_bytes())
        {
            return;
        }
        if self.lists.auto.len() >= MAX_LEARNED {
            if !self.learn_full_logged {
                log!("gecit: {} holds {MAX_LEARNED} domains, not learning more", self.cfg.auto_list.display());
                self.learn_full_logged = true;
            }
            return;
        }
        self.lists.auto.insert(name.as_bytes());
        log!("gecit: learned {name}");
        let _ = self.learned_tx.send(name);
    }

    fn housekeeping(&mut self, now: Instant) {
        if now.duration_since(self.last_prune) >= Duration::from_secs(2) {
            self.last_prune = now;
            self.flows.retain(|_, f| now.duration_since(f.seen) < FLOW_TTL);
        }
        while let Ok(l) = self.lists_rx.try_recv() {
            self.lists = l;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learnable_names() {
        assert!(learnable(b"discord.com"));
        assert!(learnable(b"a-b.example.co.uk"));
        assert!(!learnable(b"com"));
        assert!(!learnable(b"evil.com\ncom"));
        assert!(!learnable(b"1.2.3.4"));
        assert!(!learnable(b".com"));
        assert!(!learnable(b"a..com"));
    }

    #[test]
    fn injected_resets() {
        let ms = Duration::from_millis;
        assert!(injected(Duration::from_micros(1600), ms(46)), "Kablonet DPI");
        assert!(!injected(ms(174), ms(19)), "server's own reset later on");
        assert!(!injected(ms(10), ms(19)), "server answering at once");
        assert!(!injected(ms(2), ms(3)), "close server: undecidable, not learned");
    }

    #[test]
    fn hop_distance() {
        // Cloudflare (initial 64) 7 hops away: arrives with 58, seen in FORWARD as 57.
        assert_eq!(distance(57, NF_INET_FORWARD), 7);
        assert_eq!(distance(58, 1), 7);
        assert_eq!(distance(121, NF_INET_FORWARD), 7);
        assert_eq!(distance(250, 1), 6);
    }

    #[test]
    fn modes_and_exclude() {
        let mut l = Lists {
            include: HostList::default(),
            auto: HostList::default(),
            exclude: HostList::default(),
        };
        l.include.add_text("discord.com");
        l.auto.add_text("wattpad.com");
        l.exclude.add_text("status.discord.com");
        assert!(l.wants(Mode::List, b"discord.com"));
        assert!(!l.wants(Mode::List, b"wattpad.com"));
        assert!(l.wants(Mode::Auto, b"wattpad.com"));
        assert!(!l.wants(Mode::Auto, b"example.org"));
        assert!(l.wants(Mode::All, b"example.org"));
        for m in [Mode::List, Mode::Auto, Mode::All] {
            assert!(!l.wants(m, b"status.discord.com"), "exclude wins in {m:?}");
        }
    }
}
