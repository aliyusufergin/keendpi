//! IPv4/TCP parsing and fake-packet building on raw byte slices.

use std::net::Ipv4Addr;

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const ACK: u8 = 0x10;

pub struct Tcp4<'a> {
    pub raw: &'a [u8],
    pub ihl: usize,
    pub thl: usize,
    pub payload: &'a [u8],
}

impl<'a> Tcp4<'a> {
    pub fn dst(&self) -> Ipv4Addr {
        Ipv4Addr::new(self.raw[16], self.raw[17], self.raw[18], self.raw[19])
    }
    pub fn src(&self) -> Ipv4Addr {
        Ipv4Addr::new(self.raw[12], self.raw[13], self.raw[14], self.raw[15])
    }
    pub fn sport(&self) -> u16 {
        u16::from_be_bytes([self.raw[self.ihl], self.raw[self.ihl + 1]])
    }
    pub fn dport(&self) -> u16 {
        u16::from_be_bytes([self.raw[self.ihl + 2], self.raw[self.ihl + 3]])
    }
    pub fn flags(&self) -> u8 {
        self.raw[self.ihl + 13]
    }
    pub fn is_syn_fin_rst(&self) -> bool {
        self.flags() & (FIN | SYN | RST) != 0
    }
    pub fn ttl(&self) -> u8 {
        self.raw[8]
    }
}

/// Parse an IPv4 packet carrying TCP. Returns None for anything else
/// (other protocols, fragments, malformed headers).
pub fn parse_tcp4(p: &[u8]) -> Option<Tcp4<'_>> {
    if p.len() < 40 || p[0] >> 4 != 4 || p[9] != 6 {
        return None;
    }
    let ihl = ((p[0] & 0x0f) as usize) * 4;
    // Fragment offset or MF set: not a whole segment.
    if u16::from_be_bytes([p[6], p[7]]) & 0x3fff != 0 {
        return None;
    }
    // With GSO/GRO the IP total length can be 0 or smaller than the buffer; use the buffer.
    let total = match u16::from_be_bytes([p[2], p[3]]) as usize {
        0 => p.len(),
        t => t.min(p.len()),
    };
    if ihl < 20 || total < ihl + 20 {
        return None;
    }
    let thl = ((p[ihl + 12] >> 4) as usize) * 4;
    if thl < 20 || ihl + thl > total {
        return None;
    }
    Some(Tcp4 { raw: &p[..total], ihl, thl, payload: &p[ihl + thl..total] })
}

fn fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn sum16(b: &[u8]) -> u32 {
    let mut s = 0u32;
    let mut it = b.chunks_exact(2);
    for c in &mut it {
        s += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let [last] = it.remainder() {
        s += (*last as u32) << 8;
    }
    s
}

/// Build a copy of `orig`'s IP+TCP headers (options included) carrying `payload`,
/// with the given TTL and fresh checksums. Sequence number and flags are kept, so the
/// fake occupies the same stream position as the original data.
pub fn build_fake(orig: &Tcp4, payload: &[u8], ttl: u8) -> Vec<u8> {
    let hdr = orig.ihl + orig.thl;
    let total = hdr + payload.len();
    let mut p = Vec::with_capacity(total);
    p.extend_from_slice(&orig.raw[..hdr]);
    p.extend_from_slice(payload);
    p[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    p[8] = ttl;
    // IP checksum
    p[10] = 0;
    p[11] = 0;
    let c = fold(sum16(&p[..orig.ihl]));
    p[10..12].copy_from_slice(&c.to_be_bytes());
    // TCP checksum over pseudo header + segment
    let t = orig.ihl;
    p[t + 16] = 0;
    p[t + 17] = 0;
    let seg_len = total - t;
    let mut s = sum16(&p[12..20]) + 6 + seg_len as u32;
    s += sum16(&p[t..]);
    let c = fold(s);
    p[t + 16..t + 18].copy_from_slice(&c.to_be_bytes());
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    // SYN-less PSH/ACK from 192.168.1.100:40000 to 162.159.135.232:443, payload "hello".
    fn sample() -> Vec<u8> {
        let mut p = vec![
            0x45, 0, 0, 0, 0x12, 0x34, 0x40, 0, 64, 6, 0, 0, 192, 168, 1, 100, 162, 159, 135, 232,
            0x9c, 0x40, 0x01, 0xbb, 0, 0, 0, 1, 0, 0, 0, 2, 0x50, 0x18, 0xff, 0xff, 0, 0, 0, 0,
        ];
        p.extend_from_slice(b"hello");
        let l = p.len() as u16;
        p[2..4].copy_from_slice(&l.to_be_bytes());
        p
    }

    fn verify_checksums(p: &[u8]) {
        let t = parse_tcp4(p).unwrap();
        assert_eq!(fold(sum16(&p[..t.ihl])), 0, "ip checksum");
        let seg = &p[t.ihl..];
        let s = sum16(&p[12..20]) + 6 + seg.len() as u32 + sum16(seg);
        assert_eq!(fold(s), 0, "tcp checksum");
    }

    #[test]
    fn parses_and_builds() {
        let raw = sample();
        let t = parse_tcp4(&raw).unwrap();
        assert_eq!(t.payload, b"hello");
        assert_eq!(t.dst(), Ipv4Addr::new(162, 159, 135, 232));
        assert_eq!(t.src(), Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!((t.sport(), t.dport()), (40000, 443));
        assert_eq!(t.flags(), 0x18);
        assert!(!t.is_syn_fin_rst());
        let f = build_fake(&t, b"fake payload!", 3);
        let ft = parse_tcp4(&f).unwrap();
        assert_eq!(ft.payload, b"fake payload!");
        assert_eq!(ft.ttl(), 3);
        assert_eq!(&f[t.ihl + 4..t.ihl + 8], &raw[t.ihl + 4..t.ihl + 8], "same seq");
        verify_checksums(&f);
    }

    #[test]
    fn rejects_non_tcp_and_fragments() {
        let mut raw = sample();
        raw[9] = 17;
        assert!(parse_tcp4(&raw).is_none());
        let mut raw = sample();
        raw[6] = 0x20; // MF
        assert!(parse_tcp4(&raw).is_none());
    }
}
