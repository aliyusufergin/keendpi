//! Minimal TLS ClientHello handling: find the SNI, and build a fake ClientHello.

/// Outcome of looking for the SNI in the first bytes of a TCP stream.
#[derive(Debug, PartialEq, Eq)]
pub enum Sni<'a> {
    /// A ClientHello whose server_name was found.
    Found(&'a [u8]),
    /// A ClientHello without a server_name extension.
    Absent,
    /// Looks like a ClientHello, but the SNI lies beyond the bytes we have.
    Incomplete,
    /// Not the start of a TLS ClientHello.
    NotClientHello,
}

/// Bounds-checked big-endian reader.
struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> Option<usize> {
        let v = *self.b.get(self.pos)?;
        self.pos += 1;
        Some(v as usize)
    }
    fn u16(&mut self) -> Option<usize> {
        Some(self.u8()? << 8 | self.u8()?)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
}

/// Look for the SNI in a TCP payload that starts a TLS stream.
pub fn parse_sni(p: &[u8]) -> Sni<'_> {
    // Record header: type 0x16 (handshake), version 0x03 0x0?, length.
    if p.len() < 6 || p[0] != 0x16 || p[1] != 0x03 || p[5] != 0x01 {
        return Sni::NotClientHello;
    }
    // Parse only inside the first record, and only the bytes we hold.
    let rec_len = (p[3] as usize) << 8 | p[4] as usize;
    let end = p.len().min(5 + rec_len);
    let mut r = Rd { b: &p[..end], pos: 9 }; // skip record hdr (5) + handshake hdr (4)
    match find_sni(&mut r) {
        Some(Some(name)) => Sni::Found(name),
        Some(None) => Sni::Absent,
        None => Sni::Incomplete,
    }
}

fn find_sni<'a>(r: &mut Rd<'a>) -> Option<Option<&'a [u8]>> {
    r.take(2 + 32)?; // legacy_version + random
    let n = r.u8()?;
    r.take(n)?; // session id
    let n = r.u16()?;
    r.take(n)?; // cipher suites
    let n = r.u8()?;
    r.take(n)?; // compression methods
    let ext_total = r.u16()?;
    let ext_end = r.pos + ext_total;
    while r.pos + 4 <= ext_end {
        let typ = r.u16()?;
        let len = r.u16()?;
        let data = r.take(len)?;
        if typ == 0 {
            let mut s = Rd { b: data, pos: 0 };
            let _list_len = s.u16()?;
            while let Some(kind) = s.u8() {
                let n = s.u16()?;
                let name = s.take(n)?;
                if kind == 0 {
                    return Some(Some(name));
                }
            }
            return Some(None);
        }
    }
    if r.pos >= ext_end { Some(None) } else { None }
}

fn push_u16(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&(x as u16).to_be_bytes());
}

fn push_ext(v: &mut Vec<u8>, typ: u16, data: &[u8]) {
    push_u16(v, typ as usize);
    push_u16(v, data.len());
    v.extend_from_slice(data);
}

/// Build a plausible TLS 1.3 ClientHello for `sni`, padded to `target_len` bytes
/// (record included) when possible. `fill` supplies random bytes.
pub fn fake_client_hello(sni: &[u8], target_len: usize, fill: &mut dyn FnMut(&mut [u8])) -> Vec<u8> {
    let mut rnd = [0u8; 32 + 32 + 32];
    fill(&mut rnd);

    let mut ext = Vec::with_capacity(512);
    let mut sn = Vec::with_capacity(sni.len() + 5);
    push_u16(&mut sn, sni.len() + 3);
    sn.push(0);
    push_u16(&mut sn, sni.len());
    sn.extend_from_slice(sni);
    push_ext(&mut ext, 0x0000, &sn); // server_name
    push_ext(&mut ext, 0x0017, &[]); // extended_master_secret
    push_ext(&mut ext, 0xff01, &[0]); // renegotiation_info
    push_ext(&mut ext, 0x000a, &[0, 6, 0x00, 0x1d, 0x00, 0x17, 0x00, 0x18]); // groups
    push_ext(&mut ext, 0x000b, &[1, 0]); // ec_point_formats
    push_ext(&mut ext, 0x0023, &[]); // session_ticket
    push_ext(&mut ext, 0x0010, b"\x00\x0c\x02h2\x08http/1.1"); // alpn
    push_ext(&mut ext, 0x0005, &[1, 0, 0, 0, 0]); // status_request
    push_ext(
        &mut ext,
        0x000d,
        &[0, 16, 4, 3, 8, 4, 4, 1, 5, 3, 8, 5, 5, 1, 8, 6, 6, 1],
    ); // signature_algorithms
    let mut ks = vec![0, 36, 0x00, 0x1d, 0, 32];
    ks.extend_from_slice(&rnd[64..96]);
    push_ext(&mut ext, 0x0033, &ks); // key_share x25519
    push_ext(&mut ext, 0x002d, &[1, 1]); // psk_key_exchange_modes
    push_ext(&mut ext, 0x002b, &[4, 3, 4, 3, 3]); // supported_versions

    let mut body = Vec::with_capacity(target_len);
    body.extend_from_slice(&[3, 3]);
    body.extend_from_slice(&rnd[..32]);
    body.push(32);
    body.extend_from_slice(&rnd[32..64]);
    let suites: [u16; 9] = [0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8];
    push_u16(&mut body, suites.len() * 2);
    for s in suites {
        push_u16(&mut body, s as usize);
    }
    body.extend_from_slice(&[1, 0]);

    // Pad with the padding extension (21) to reach target_len overall.
    let fixed = 5 + 4 + body.len() + 2 + ext.len();
    if target_len >= fixed + 4 {
        let pad = target_len - fixed - 4;
        push_ext(&mut ext, 0x0015, &vec![0u8; pad]);
    }
    push_u16(&mut body, ext.len());
    body.extend_from_slice(&ext);

    let mut out = Vec::with_capacity(9 + body.len());
    out.extend_from_slice(&[0x16, 3, 1]);
    push_u16(&mut out, body.len() + 4);
    out.push(1);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero(b: &mut [u8]) {
        b.fill(0x42)
    }

    #[test]
    fn fake_roundtrips_through_parser() {
        let ch = fake_client_hello(b"www.google.com", 517, &mut zero);
        assert_eq!(ch.len(), 517);
        assert_eq!(parse_sni(&ch), Sni::Found(b"www.google.com"));
    }

    #[test]
    fn truncated_is_incomplete() {
        let ch = fake_client_hello(b"discord.com", 517, &mut zero);
        // Cut inside the cipher suites, before extensions.
        assert_eq!(parse_sni(&ch[..60]), Sni::Incomplete);
        // Cut inside the SNI itself.
        let pos = ch.windows(7).position(|w| w == b"discord").unwrap();
        assert_eq!(parse_sni(&ch[..pos + 3]), Sni::Incomplete);
    }

    #[test]
    fn sni_as_last_extension() {
        // Chrome may place server_name last, after a ~1.2 KB key share. Build such a hello
        // by moving the SNI extension of the OpenSSL capture to the end.
        let ch = include_bytes!("../tests/data/openssl_clienthello_discord.bin").to_vec();
        let pos = ch.windows(4).position(|w| w == [0, 0, 0, 16]).unwrap(); // type 0, len 16
        let sni_ext: Vec<u8> = ch[pos..pos + 4 + 16].to_vec();
        let mut moved = ch[..pos].to_vec();
        moved.extend_from_slice(&ch[pos + 20..]);
        moved.extend_from_slice(&sni_ext);
        assert_eq!(parse_sni(&moved), Sni::Found(b"discord.com"));
        assert_eq!(parse_sni(&moved[..1400]), Sni::Incomplete);
    }

    #[test]
    fn non_tls_rejected() {
        assert_eq!(parse_sni(b"GET / HTTP/1.1\r\n"), Sni::NotClientHello);
        assert_eq!(parse_sni(&[0x17, 3, 3, 0, 5, 1, 0]), Sni::NotClientHello);
    }

    #[test]
    fn real_openssl_hello() {
        // A full ClientHello generated by Python 3 / OpenSSL 3 for SNI "discord.com"
        // (about 1.5 KB with a post-quantum key share).
        let ch = include_bytes!("../tests/data/openssl_clienthello_discord.bin");
        assert_eq!(parse_sni(ch), Sni::Found(b"discord.com"));
        // As it arrives in a first TCP segment of 1400 bytes the SNI is still visible.
        assert_eq!(parse_sni(&ch[..1400]), Sni::Found(b"discord.com"));
    }
}
