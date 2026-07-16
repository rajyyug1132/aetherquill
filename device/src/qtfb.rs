//! Native qtfb client: SOCK_SEQPACKET protocol + shared-memory framebuffer.
//! Ported from riddle's qtfb.rs (MIT, MaximeRivest/riddle) with reMarkable 2
//! constants; wire format verified against rm-appload src/qtfb/common.h.
//!
//!   ClientMessage  = 24 bytes, type:u8 @0, payload @4
//!   ServerMessage  = 24 bytes, type:u8 @0, payload @4
//!
//! NOTE: riddle's offsets (payload @8, shmSize:u64) are for the Paper Pro's
//! 64-bit xochitl. RM2 xochitl is armv7 (32-bit): size_t=4, union align=4,
//! so server payload starts @4 and shmSize is u32 @8. Verified against
//! rm-appload src/qtfb/common.h struct layout + live failure on device.

use std::io;
use std::os::fd::RawFd;

pub const MESSAGE_INITIALIZE: u8 = 0;
pub const MESSAGE_UPDATE: u8 = 1;
pub const MESSAGE_TERMINATE: u8 = 3;
pub const MESSAGE_USERINPUT: u8 = 4;

pub const UPDATE_ALL: i32 = 0;
pub const UPDATE_PARTIAL: i32 = 1;

/// FBFMT_RM2FB: native reMarkable 2, 1404x1872, RGB565 (2 bytes/pixel).
pub const FBFMT_RM2FB: u8 = 0;
pub const RM2_WIDTH: usize = 1404;
pub const RM2_HEIGHT: usize = 1872;
pub const RM2_BPP: usize = 2;

// Input event types (server -> client).
pub const INPUT_TOUCH_PRESS: i32 = 0x10;
pub const INPUT_PEN_PRESS: i32 = 0x20;
pub const INPUT_PEN_RELEASE: i32 = 0x21;
pub const INPUT_PEN_UPDATE: i32 = 0x22;

const SOCKET_PATH: &str = "/tmp/qtfb.sock";

#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    pub input_type: i32,
    #[allow(dead_code)]
    pub dev_id: i32,
    pub x: i32,
    pub y: i32,
    #[allow(dead_code)]
    pub d: i32,
}

pub struct QtfbClient {
    fd: RawFd,
    shm: *mut u8,
    shm_len: usize,
}

// The raw pointer is to a MAP_SHARED region; single-threaded app.
unsafe impl Send for QtfbClient {}

impl QtfbClient {
    /// Connect and initialize as an RM2-format framebuffer window.
    pub fn connect(key: i32) -> io::Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (i, b) in SOCKET_PATH.bytes().enumerate() {
            addr.sun_path[i] = b as libc::c_char;
        }
        let rc = unsafe {
            libc::connect(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t)
        };
        if rc != 0 {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }

        // MESSAGE_INITIALIZE: key i32 @4, format u8 @8.
        let mut msg = [0u8; 24];
        msg[0] = MESSAGE_INITIALIZE;
        msg[4..8].copy_from_slice(&key.to_le_bytes());
        msg[8] = FBFMT_RM2FB;
        send_all(fd, &msg)?;

        // Init reply (armv7): shmKey i32 @4, shmSize u32 @8. Server closing
        // without replying (recv == 0) means init was rejected.
        let mut reply = [0u8; 32];
        let n = unsafe { libc::recv(fd, reply.as_mut_ptr() as *mut libc::c_void, 32, 0) };
        if n < 12 {
            unsafe { libc::close(fd) };
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "qtfb server rejected init (no/short reply)"));
        }
        let shm_key = i32::from_le_bytes(reply[4..8].try_into().unwrap());
        let shm_size = u32::from_le_bytes(reply[8..12].try_into().unwrap()) as usize;

        let shm_path = format!("/dev/shm/qtfb_{}\0", shm_key);
        let shm_fd = unsafe { libc::open(shm_path.as_ptr() as *const libc::c_char, libc::O_RDWR) };
        if shm_fd < 0 {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        let ptr = unsafe {
            libc::mmap(std::ptr::null_mut(), shm_size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, shm_fd, 0)
        };
        unsafe { libc::close(shm_fd) };
        if ptr == libc::MAP_FAILED {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }

        if shm_size < RM2_WIDTH * RM2_HEIGHT * RM2_BPP {
            unsafe { libc::close(fd) };
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("shm too small: {shm_size}")));
        }

        // Non-blocking: the event loop drains input events opportunistically.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        Ok(Self { fd, shm: ptr as *mut u8, shm_len: shm_size })
    }

    pub fn raw_fd(&self) -> RawFd {
        self.fd
    }

    /// The framebuffer as RGB565 pixels (white = 0xFFFF, black = 0x0000).
    pub fn pixels(&mut self) -> &mut [u16] {
        unsafe { std::slice::from_raw_parts_mut(self.shm as *mut u16, RM2_WIDTH * RM2_HEIGHT) }
    }

    fn send_msg(&self, msg: &[u8; 24]) -> io::Result<()> {
        send_all(self.fd, msg)
    }

    pub fn update_all(&self) -> io::Result<()> {
        let mut msg = [0u8; 24];
        msg[0] = MESSAGE_UPDATE;
        msg[4..8].copy_from_slice(&UPDATE_ALL.to_le_bytes());
        self.send_msg(&msg)
    }

    pub fn update_partial(&self, x: i32, y: i32, w: i32, h: i32) -> io::Result<()> {
        let mut msg = [0u8; 24];
        msg[0] = MESSAGE_UPDATE;
        msg[4..8].copy_from_slice(&UPDATE_PARTIAL.to_le_bytes());
        msg[8..12].copy_from_slice(&x.to_le_bytes());
        msg[12..16].copy_from_slice(&y.to_le_bytes());
        msg[16..20].copy_from_slice(&w.to_le_bytes());
        msg[20..24].copy_from_slice(&h.to_le_bytes());
        self.send_msg(&msg)
    }

    pub fn terminate(&self) {
        let mut msg = [0u8; 24];
        msg[0] = MESSAGE_TERMINATE;
        let _ = self.send_msg(&msg);
    }

    /// Drain pending server messages. Returns input events, or Err on
    /// disconnect (window closed -> we must exit).
    pub fn drain_events(&self) -> io::Result<Vec<InputEvent>> {
        let mut out = Vec::new();
        loop {
            let mut buf = [0u8; 32];
            let n = unsafe { libc::recv(self.fd, buf.as_mut_ptr() as *mut libc::c_void, 32, 0) };
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::ConnectionReset, "qtfb socket closed"));
            }
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::WouldBlock {
                    return Ok(out);
                }
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            // UserInputContents (armv7): inputType @4, devId @8, x @12, y @16, d @20.
            if buf[0] == MESSAGE_USERINPUT && n >= 24 {
                out.push(InputEvent {
                    input_type: i32::from_le_bytes(buf[4..8].try_into().unwrap()),
                    dev_id: i32::from_le_bytes(buf[8..12].try_into().unwrap()),
                    x: i32::from_le_bytes(buf[12..16].try_into().unwrap()),
                    y: i32::from_le_bytes(buf[16..20].try_into().unwrap()),
                    d: i32::from_le_bytes(buf[20..24].try_into().unwrap()),
                });
            }
        }
    }
}

impl Drop for QtfbClient {
    fn drop(&mut self) {
        self.terminate();
        unsafe {
            libc::munmap(self.shm as *mut libc::c_void, self.shm_len);
            libc::close(self.fd);
        }
    }
}

fn send_all(fd: RawFd, buf: &[u8]) -> io::Result<()> {
    loop {
        let n = unsafe { libc::send(fd, buf.as_ptr() as *const libc::c_void, buf.len(), 0) };
        if n == buf.len() as isize {
            return Ok(());
        }
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            // Non-blocking socket: retry sends briefly rather than dropping a
            // protocol message (updates are small and the server drains fast).
            if e.kind() == io::ErrorKind::WouldBlock {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            return Err(e);
        }
        return Err(io::Error::new(io::ErrorKind::WriteZero, "short send"));
    }
}
