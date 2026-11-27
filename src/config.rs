//! Configuration file: shell-style `KEY=value` lines, so the init script can `.` the same file.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only domains on the manual lists.
    List,
    /// Manual lists plus domains learned from block signals.
    Auto,
    /// Every TLS/HTTP connection (minus the exclude list).
    All,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub queue: u16,
    /// Packets carrying this mark bit are skipped by the rules (our own fakes, check's direct probe).
    pub mark: u32,
    /// Packets carrying this mark bit get the bypass regardless of lists (check's bypass probe).
    pub force_mark: u32,
    /// TTL of fake packets as they leave the router; capped per flow below the server's distance.
    pub ttl: u8,
    pub fake_host: String,
    /// Include lists: files, or directories whose `*.txt` files are read.
    pub lists: Vec<PathBuf>,
    pub exclude: Vec<PathBuf>,
    /// Where `auto` mode stores learned domains.
    pub auto_list: PathBuf,
    pub http: bool,
    /// Read by the scripts: whether the service starts, and the WAN interface(s).
    pub enabled: bool,
    pub wan: String,
    /// Log every handled connection (troubleshooting).
    pub verbose: bool,
}

pub const DEFAULT_PATH: &str = "/opt/etc/keendpi/keendpi.conf";

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Auto,
            queue: 200,
            mark: 0x4000_0000,
            force_mark: 0x2000_0000,
            ttl: 3,
            fake_host: "www.google.com".into(),
            lists: vec!["/opt/etc/keendpi/lists".into()],
            exclude: vec!["/opt/etc/keendpi/exclude.txt".into()],
            auto_list: "/opt/etc/keendpi/auto.txt".into(),
            http: true,
            enabled: true,
            wan: "auto".into(),
            verbose: false,
        }
    }
}

/// Strip shell quoting and a trailing comment from a value.
fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(rest) = v.strip_prefix(q) {
            return rest.split(q).next().unwrap_or("");
        }
    }
    v.split(" #").next().unwrap_or("").trim()
}

fn hex_or_dec(v: &str) -> Result<u32, String> {
    match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).map_err(|e| e.to_string()),
        None => v.parse().map_err(|e: std::num::ParseIntError| e.to_string()),
    }
}

fn yes_no(v: &str) -> Option<bool> {
    match v {
        "1" | "yes" | "on" => Some(true),
        "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn paths(v: &str) -> Vec<PathBuf> {
    v.split_whitespace().map(PathBuf::from).collect()
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c = Config::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let err = |e: String| format!("line {}: {e}", n + 1);
            let (k, v) = line.split_once('=').ok_or_else(|| err(format!("expected KEY=value, got {line:?}")))?;
            let v = unquote(v);
            match k.trim() {
                "MODE" => {
                    c.mode = match v {
                        "list" => Mode::List,
                        "auto" => Mode::Auto,
                        "all" => Mode::All,
                        _ => return Err(err(format!("MODE must be list, auto or all, not {v:?}"))),
                    }
                }
                "QUEUE" => c.queue = v.parse().map_err(|e| err(format!("QUEUE: {e}")))?,
                "MARK" => c.mark = hex_or_dec(v).map_err(|e| err(format!("MARK: {e}")))?,
                "FORCE_MARK" => c.force_mark = hex_or_dec(v).map_err(|e| err(format!("FORCE_MARK: {e}")))?,
                "TTL" => c.ttl = v.parse().map_err(|e| err(format!("TTL: {e}")))?,
                "FAKE_HOST" => c.fake_host = v.to_string(),
                "LISTS" => c.lists = paths(v),
                "EXCLUDE" => c.exclude = paths(v),
                "AUTO_LIST" => c.auto_list = v.into(),
                "HTTP" => c.http = yes_no(v).ok_or_else(|| err(format!("HTTP must be yes or no, not {v:?}")))?,
                "VERBOSE" => c.verbose = yes_no(v).ok_or_else(|| err(format!("VERBOSE must be yes or no, not {v:?}")))?,
                "ENABLED" => c.enabled = yes_no(v).ok_or_else(|| err(format!("ENABLED must be yes or no, not {v:?}")))?,
                "WAN" => {
                    if v.is_empty() || !v.split_whitespace().all(|i| i.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))) {
                        return Err(err(format!("WAN must be auto or interface names, not {v:?}")));
                    }
                    c.wan = v.split_whitespace().collect::<Vec<_>>().join(" ");
                }
                k => return Err(err(format!("unknown key {k}"))),
            }
        }
        if c.ttl == 0 {
            return Err("TTL must be at least 1".into());
        }
        if c.mark == 0 || c.force_mark == 0 || c.mark & c.force_mark != 0 {
            return Err("MARK and FORCE_MARK must be distinct non-zero bits".into());
        }
        if c.fake_host.is_empty() || c.fake_host.len() > 200 {
            return Err("FAKE_HOST must be a host name".into());
        }
        Ok(c)
    }

    /// The settings the shell scripts need, as `KEY='value'` lines for `eval`. The scripts never
    /// source the file themselves, so both sides read it the same way (CRLF, spaces, quotes).
    pub fn shell_vars(&self) -> String {
        format!(
            "ENABLED={}\nVERBOSE={}\nWAN='{}'\nQUEUE={}\nMARK={:#x}\nHTTP={}\nLEARN={}\n",
            if self.enabled { "yes" } else { "no" },
            if self.verbose { "yes" } else { "no" },
            self.wan,
            self.queue,
            self.mark,
            if self.http { "yes" } else { "no" },
            if self.mode == Mode::Auto { "yes" } else { "no" },
        )
    }

    pub fn load(path: &str) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Config::parse(&text).map_err(|e| format!("{path}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shell_style() {
        let c = Config::parse(
            "# comment\nMODE=list\nTTL=4  # four\nMARK=0x10000000\nFAKE_HOST=\"www.example.com\"\nLISTS='/a /b'\nWAN=ppp0\nHTTP=no\n",
        )
        .unwrap();
        assert_eq!(c.mode, Mode::List);
        assert_eq!(c.ttl, 4);
        assert_eq!(c.mark, 0x1000_0000);
        assert_eq!(c.fake_host, "www.example.com");
        assert_eq!(c.lists, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert!(!c.http);
        assert_eq!(c.wan, "ppp0");
    }

    #[test]
    fn crlf_and_spaces() {
        let c = Config::parse("MODE = list\r\nWAN=\"ppp0  eth3\"\r\nENABLED=no\r\n").unwrap();
        assert_eq!(c.mode, Mode::List);
        assert!(!c.enabled);
        assert!(c.shell_vars().contains("WAN='ppp0 eth3'\n"));
        assert!(c.shell_vars().contains("LEARN=no\n"));
        assert!(Config::parse("WAN=ppp0;reboot").is_err());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Config::parse("MODE=fast").is_err());
        assert!(Config::parse("TTL=0").is_err());
        assert!(Config::parse("BOGUS=1").is_err());
        assert!(Config::parse("MARK=0x20000000").is_err(), "same bit as FORCE_MARK");
        assert!(Config::parse("just text").is_err());
    }
}
