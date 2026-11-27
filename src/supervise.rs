//! The service process: runs the worker as a child and restarts it when it exits or hangs.
//!
//! A hung worker (stopped, deadlocked) leaves packets waiting in its queue. The kernel only
//! accepts them unseen once the queue is full, which on a quiet network can take long. So the
//! supervisor watches the queue's backlog in /proc and kills the worker when packets sit there
//! for about a second. The kernel then drops the waiting ClientHellos, the clients retransmit
//! them, and the new worker (or, until it is up, `--queue-bypass`) lets them through.

use crate::config::Config;
use crate::engine::Lists;
use crate::log;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

const TICK: Duration = Duration::from_millis(250);
/// Consecutive ticks in which packets waited and none was taken that mean "hung".
const STUCK_TICKS: u32 = 4;

/// (packets waiting, packets ever queued) for queue `q`, or None when no process is bound to it.
pub fn queue_stats(q: u16) -> Option<(u32, u32)> {
    let text = std::fs::read_to_string("/proc/net/netfilter/nfnetlink_queue").ok()?;
    parse_stats(&text, q)
}

fn parse_stats(text: &str, q: u16) -> Option<(u32, u32)> {
    // Columns: queue_number peer_portid queue_total copy_mode copy_range queue_dropped
    //          user_dropped id_sequence 1
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.len() >= 8 && f[0].parse::<u16>().ok()? == q).then_some(())?;
        Some((f[2].parse().ok()?, f[7].parse().ok()?))
    })
}

/// Packets the worker took off the queue between two samples. The kernel numbers every packet
/// it queues (id_sequence), so taken = newly queued - growth of the backlog. A busy but healthy
/// worker always takes some; a hung one takes none, even while the queue sits full.
fn taken(prev: (u32, u32), now: (u32, u32)) -> i64 {
    let queued = now.1.wrapping_sub(prev.1) as i64;
    queued - (now.0 as i64 - prev.0 as i64)
}

fn spawn(cfg_path: &str, verbose: bool) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    let mut c = Command::new(exe);
    c.arg("worker").arg("-c").arg(cfg_path);
    if verbose {
        c.arg("-v");
    }
    if crate::logging::syslog_on() {
        c.arg("--syslog");
    }
    // The worker must not outlive the supervisor.
    unsafe {
        c.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    c.spawn()
}

fn kill(child: &mut Child, sig: libc::c_int) {
    unsafe {
        libc::kill(child.id() as libc::pid_t, sig);
    }
}

/// Wait for a killed child without blocking forever: a worker stuck in the kernel (say, on a
/// hung USB disk) dies only when that I/O returns.
fn reap(child: &mut Child) {
    let mut logged = false;
    let t = Instant::now();
    loop {
        if !matches!(child.try_wait(), Ok(None)) || STOP.load(Ordering::Relaxed) {
            return;
        }
        if !logged && t.elapsed() > Duration::from_secs(3) {
            log!("keendpi: worker does not die (stuck in the kernel?); waiting for it");
            logged = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn stop_child(child: &mut Child) {
    kill(child, libc::SIGTERM);
    for _ in 0..20 {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    kill(child, libc::SIGKILL);
    reap(child);
}

pub fn run(cfg_path: &str, verbose: bool) -> Result<(), String> {
    // Check config and lists here, so a mistake is reported once instead of as a restart loop.
    let cfg = Config::load(cfg_path)?;
    let lists = Lists::load(&cfg)?;
    log!("keendpi {}: mode {:?}, {}", env!("CARGO_PKG_VERSION"), cfg.mode, lists.summary());
    drop(lists);
    if queue_stats(cfg.queue).is_some() {
        return Err(format!("queue {} is already in use by another process", cfg.queue));
    }
    unsafe {
        let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, h);
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }

    let mut backoff = Duration::from_millis(500);
    loop {
        let started = Instant::now();
        let mut child = spawn(cfg_path, verbose).map_err(|e| format!("starting worker: {e}"))?;
        let mut stuck = 0;
        let mut last: Option<(u32, u32)> = None;
        let hung = loop {
            std::thread::sleep(TICK);
            if STOP.load(Ordering::Relaxed) {
                stop_child(&mut child);
                log!("keendpi: stopped");
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(st)) => {
                    log!("keendpi: worker {st}");
                    break false;
                }
                Ok(None) => {}
                Err(e) => log!("keendpi: wait: {e}"),
            }
            let now = queue_stats(cfg.queue);
            stuck = match (last, now) {
                (Some(p), Some(n)) if n.0 > 0 && taken(p, n) <= 0 => stuck + 1,
                _ => 0,
            };
            last = now;
            if stuck >= STUCK_TICKS {
                log!("keendpi: worker not taking packets ({} waiting), restarting it", now.map_or(0, |n| n.0));
                kill(&mut child, libc::SIGKILL);
                reap(&mut child);
                break true;
            }
        };
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_millis(500);
        }
        // A hang is restarted at once (clients are retransmitting into the queue right now);
        // repeated quick exits back off up to 30 s.
        if !hung {
            let until = Instant::now() + backoff;
            while Instant::now() < until {
                if STOP.load(Ordering::Relaxed) {
                    log!("keendpi: stopped");
                    return Ok(());
                }
                std::thread::sleep(TICK);
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_from_proc() {
        let t = "  100  12345     0 2  4096     0     0        5  1\n  200  23456     3 2  4096     0     0       42  1\n";
        assert_eq!(parse_stats(t, 200), Some((3, 42)));
        assert_eq!(parse_stats(t, 100), Some((0, 5)));
        assert_eq!(parse_stats(t, 7), None);
        assert_eq!(parse_stats("", 200), None);
    }

    #[test]
    fn progress() {
        assert_eq!(taken((0, 10), (2, 20)), 8, "busy and healthy");
        assert_eq!(taken((64, 100), (64, 100)), 0, "full and hung: nothing new numbered");
        assert_eq!(taken((1, 10), (3, 12)), 0, "hung, still filling");
        assert_eq!(taken((5, u32::MAX), (0, 4)), 10, "id wraps");
    }
}
