//! Plain HTTP: find the Host header of a request, and build a fake request.

/// Outcome of looking for the Host header in the first segment of an HTTP request.
#[derive(Debug, PartialEq, Eq)]
pub enum Host<'a> {
    Found(&'a [u8]),
    /// A request whose headers continue beyond this segment.
    Incomplete,
    /// Headers complete without a Host, or not an HTTP request.
    None,
}

const METHODS: [&[u8]; 7] = [b"GET ", b"POST ", b"HEAD ", b"PUT ", b"DELETE ", b"OPTIONS ", b"PATCH "];

pub fn is_request(p: &[u8]) -> bool {
    METHODS.iter().any(|m| p.starts_with(m))
}

pub fn parse_host(p: &[u8]) -> Host<'_> {
    if !is_request(p) {
        return Host::None;
    }
    let mut lines = p.split(|&c| c == b'\n');
    lines.next(); // request line
    let mut complete = false;
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            complete = true;
            break;
        }
        if line.len() > 5 && line[..5].eq_ignore_ascii_case(b"host:") {
            let v = line[5..].trim_ascii();
            // Drop a port; keep IPv6 literals intact.
            let v = match v.iter().rposition(|&c| c == b':') {
                Some(i) if !v.contains(&b']') || v[..i].ends_with(b"]") => &v[..i],
                _ => v,
            };
            return Host::Found(v);
        }
    }
    // The last line may be a cut-off header, so only a blank line proves "no Host".
    if complete { Host::None } else { Host::Incomplete }
}

pub fn fake_request(host: &str) -> Vec<u8> {
    format!("GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Mozilla/5.0\r\nAccept: */*\r\n\r\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_host() {
        assert_eq!(parse_host(b"GET / HTTP/1.1\r\nHost: discord.com\r\n\r\n"), Host::Found(b"discord.com"));
        assert_eq!(parse_host(b"GET /x HTTP/1.1\r\nUser-Agent: a\r\nhOsT:discord.com:8080\r\n\r\n"), Host::Found(b"discord.com"));
        assert_eq!(parse_host(b"GET / HTTP/1.1\r\nHost: [::1]:80\r\n\r\n"), Host::Found(b"[::1]"));
        assert_eq!(parse_host(b"GET / HTTP/1.1\r\nUser-Agent: a\r\nHo"), Host::Incomplete);
        assert_eq!(parse_host(b"GET / HTTP/1.0\r\n\r\n"), Host::None);
        assert_eq!(parse_host(b"\x16\x03\x01"), Host::None);
        assert_eq!(parse_host(&fake_request("www.google.com")), Host::Found(b"www.google.com"));
    }
}
