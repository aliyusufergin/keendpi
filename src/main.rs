//! gecit: NFQUEUE DPI-circumvention engine for Keenetic routers.
//!
//! For a TLS ClientHello or HTTP request to a blocked domain, send one fake request (harmless
//! name, TTL too low to reach the server) at the same sequence number, then accept the
//! original unchanged. The DPI box inspects the fake and lets the connection pass.

mod check;
mod config;
mod engine;
mod hostlist;
mod http;
mod logging;
mod packet;
mod rawsock;
mod supervise;
mod tls;

use std::process::ExitCode;

/// Log one line to stderr or syslog, never panicking (see logging.rs).
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        $crate::logging::line(format_args!($($t)*));
    }};
}

const USAGE: &str = "usage:
  gecit run      [-c FILE] [-v] [--syslog]  run the service (supervises the worker)
  gecit check    [-c FILE] [DOMAIN...]      test each listed (or given) domain, direct and with gecit
  gecit validate [-c FILE]                  check the configuration and lists, then exit
  gecit shellvars [-c FILE]                 print the settings the scripts use (KEY='value')
  gecit version
options:
  -c FILE    configuration file (default /opt/etc/gecit/gecit.conf)
  -v         log every handled connection
  --syslog   log to the system log instead of stderr";

struct Args {
    cmd: String,
    conf: String,
    verbose: bool,
    syslog: bool,
    rest: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(USAGE)?;
    let mut a = Args { cmd, conf: config::DEFAULT_PATH.into(), verbose: false, syslog: false, rest: vec![] };
    while let Some(x) = it.next() {
        match x.as_str() {
            "-c" => a.conf = it.next().ok_or("-c needs a file")?,
            "-v" => a.verbose = true,
            "--syslog" => a.syslog = true,
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => a.rest.push(x),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            log!("{e}");
            return ExitCode::from(2);
        }
    };
    if a.syslog {
        if let Err(e) = logging::use_syslog() {
            log!("gecit: /dev/log: {e}; logging to stderr");
        }
    }
    let r = match a.cmd.as_str() {
        "run" => supervise::run(&a.conf, a.verbose),
        "validate" => config::Config::load(&a.conf).and_then(|c| engine::Lists::load(&c)).map(|l| {
            println!("{}: ok, {}", a.conf, l.summary());
        }),
        "shellvars" => config::Config::load(&a.conf).map(|c| print!("{}", c.shell_vars())),
        "worker" => config::Config::load(&a.conf)
            .and_then(|c| engine::Engine::new(c, a.verbose))
            .and_then(|e| e.run().map_err(|e| e.to_string())),
        "check" => config::Config::load(&a.conf).and_then(|c| check::run(&c, a.rest)),
        "version" | "--version" | "-V" => {
            println!("gecit {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        c => Err(format!("unknown command {c}\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log!("gecit: {e}");
            ExitCode::FAILURE
        }
    }
}
