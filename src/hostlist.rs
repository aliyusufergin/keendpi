//! Domain list with suffix matching: "discord.com" matches "cdn.discord.com".

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Default, Clone)]
pub struct HostList {
    names: HashSet<Vec<u8>>,
}

impl HostList {
    /// Add entries from list-file text: one domain per line, `#` comments, blanks ignored.
    pub fn add_text(&mut self, text: &str) {
        for line in text.lines() {
            let d = line.split('#').next().unwrap_or("").trim().trim_start_matches("*.").trim_end_matches('.');
            if !d.is_empty() {
                self.names.insert(d.to_ascii_lowercase().into_bytes());
            }
        }
    }

    pub fn insert(&mut self, name: &[u8]) {
        self.names.insert(name.to_ascii_lowercase());
    }

    /// The entries, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.names.iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
        v.sort();
        v
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn matches(&self, host: &[u8]) -> bool {
        let h = host.to_ascii_lowercase();
        let mut s: &[u8] = &h;
        loop {
            if self.names.contains(s) {
                return true;
            }
            match s.iter().position(|&c| c == b'.') {
                Some(i) => s = &s[i + 1..],
                None => return false,
            }
        }
    }
}

/// The files behind a set of list paths: files as given, directories as their `*.txt` files.
fn expand(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = vec![];
    for p in paths {
        match std::fs::read_dir(p) {
            Ok(rd) => {
                let mut v: Vec<PathBuf> =
                    rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "txt")).collect();
                v.sort();
                out.extend(v);
            }
            Err(_) => out.push(p.clone()),
        }
    }
    out
}

/// What the files looked like when read, to notice edits cheaply.
pub type Stamp = Vec<(PathBuf, Option<(SystemTime, u64)>)>;

pub fn stamp(paths: &[PathBuf]) -> Stamp {
    expand(paths)
        .into_iter()
        .map(|f| {
            let m = std::fs::metadata(&f).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
            (f, m)
        })
        .collect()
}

/// Read every list file behind `paths`. Missing files count as empty; other read errors are returned.
pub fn load(paths: &[PathBuf]) -> Result<HostList, String> {
    let mut l = HostList::default();
    for f in expand(paths) {
        // Lossy, so a stray non-UTF-8 byte (say, a cp1254 comment) can't make a list unreadable.
        match std::fs::read(&f) {
            Ok(t) => l.add_text(&String::from_utf8_lossy(&t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", f.display())),
        }
    }
    Ok(l)
}

/// Append one learned domain to a list file.
pub fn append(file: &Path, name: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    writeln!(f, "{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_match() {
        let mut l = HostList::default();
        l.add_text("# c\ndiscord.com\n*.discord.gg  # x\n\nPasteBin.com.\n");
        assert_eq!(l.len(), 3);
        assert!(l.matches(b"discord.com"));
        assert!(l.matches(b"CDN.Discord.com"));
        assert!(l.matches(b"x.discord.gg"));
        assert!(l.matches(b"pastebin.com"));
        assert!(!l.matches(b"notdiscord.com"));
        assert!(!l.matches(b"com"));
    }
}
