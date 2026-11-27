//! Log sink: stderr by default, or the system log (/dev/log) with `--syslog`, which on Keenetic
//! shows up in the web interface's log and never fills the USB disk.

use std::os::unix::net::UnixDatagram;
use std::sync::{Mutex, OnceLock};

static SYSLOG: OnceLock<Mutex<UnixDatagram>> = OnceLock::new();

fn connect() -> std::io::Result<UnixDatagram> {
    let s = UnixDatagram::unbound()?;
    s.connect("/dev/log")?;
    // A busy syslogd must never stall the packet loop: drop the line instead.
    s.set_nonblocking(true)?;
    Ok(s)
}

pub fn use_syslog() -> std::io::Result<()> {
    let _ = SYSLOG.set(Mutex::new(connect()?));
    Ok(())
}

pub fn syslog_on() -> bool {
    SYSLOG.get().is_some()
}

/// Write one line. Never panics and never blocks for long: a full or missing log just loses it.
pub fn line(msg: std::fmt::Arguments) {
    match SYSLOG.get() {
        Some(m) => {
            let text = msg.to_string();
            let text = text.strip_prefix("keendpi: ").unwrap_or(&text);
            // RFC 3164: <30> = facility daemon, severity info; then the timestamp, which the
            // Keenetic log parser needs to find the tag.
            let line = format!("<30>{} keendpi[{}]: {text}", stamp(), std::process::id());
            let Ok(mut s) = m.lock() else { return };
            if let Err(e) = s.send(line.as_bytes()) {
                // syslogd restarted: reconnect once. (A full buffer, WouldBlock, just loses the line.)
                if e.kind() != std::io::ErrorKind::WouldBlock {
                    if let Ok(n) = connect() {
                        *s = n;
                        let _ = s.send(line.as_bytes());
                    }
                }
            }
        }
        None => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "{msg}");
        }
    }
}

/// "Nov 27 22:00:56" in local time.
fn stamp() -> String {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let m = MON.get(tm.tm_mon as usize).unwrap_or(&"Jan");
        format!("{m} {:2} {:02}:{:02}:{:02}", tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}
