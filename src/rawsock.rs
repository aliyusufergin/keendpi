//! Raw IPv4 socket for sending crafted packets, marked so our rules skip them.

use std::io;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub struct RawSock {
    fd: OwnedFd,
}

impl RawSock {
    pub fn new(mark: u32) -> io::Result<Self> {
        // IPPROTO_RAW implies IP_HDRINCL: we supply the whole IP header.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::IPPROTO_RAW) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let r = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_MARK,
                &mark as *const u32 as *const libc::c_void,
                4,
            )
        };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RawSock { fd })
    }

    pub fn send(&self, pkt: &[u8], dst: Ipv4Addr) -> io::Result<()> {
        let sa = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: 0,
            sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(dst.octets()) },
            sin_zero: [0; 8],
        };
        let r = unsafe {
            libc::sendto(
                self.fd.as_raw_fd(),
                pkt.as_ptr() as *const libc::c_void,
                pkt.len(),
                0,
                &sa as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };
        if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
}

/// Fill a buffer from the kernel RNG (getrandom(2), Linux >= 3.17).
pub fn fill_random(buf: &mut [u8]) {
    let mut done = 0;
    while done < buf.len() {
        let r = unsafe { libc::getrandom(buf[done..].as_mut_ptr() as *mut libc::c_void, buf.len() - done, 0) };
        if r <= 0 {
            // Should never happen; fall back to something non-constant.
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            for (i, b) in buf[done..].iter_mut().enumerate() {
                *b = (t >> ((i % 16) * 8)) as u8 ^ i as u8;
            }
            return;
        }
        done += r as usize;
    }
}
