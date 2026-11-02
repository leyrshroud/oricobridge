#![allow(non_snake_case, non_camel_case_types)]

// Must be packed: without it x64 pads bt_addr and the struct becomes 40 bytes,
// and connect() fails with WSAEADDRNOTAVAIL (10049).
#[repr(C, packed)]
pub struct SOCKADDR_BTH {
    pub addressFamily: u16,
    pub btAddr: u64,
    pub serviceClassId: [u8; 16],
    pub port: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

pub type SOCKET = usize;
pub type HKEY = usize;

pub const INVALID_SOCKET: SOCKET = !0;
pub const AF_BTH: i32 = 32;
pub const SOCK_STREAM: i32 = 1;
pub const BTHPROTO_RFCOMM: i32 = 3;
pub const SOL_SOCKET: i32 = 0xFFFF;
pub const SO_SNDTIMEO: i32 = 0x1005;
pub const SO_RCVTIMEO: i32 = 0x1006;

pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002;
pub const KEY_READ: u32 = 0x20019;

#[link(name = "ws2_32")]
unsafe extern "system" {
    pub fn WSAStartup(wVersionRequested: u16, lpWSAData: *mut u8) -> i32;
    pub fn WSAGetLastError() -> i32;
    pub fn socket(af: i32, ty: i32, protocol: i32) -> SOCKET;
    pub fn connect(s: SOCKET, name: *const SOCKADDR_BTH, namelen: i32) -> i32;
    pub fn send(s: SOCKET, buf: *const u8, len: i32, flags: i32) -> i32;
    pub fn closesocket(s: SOCKET) -> i32;
    pub fn setsockopt(s: SOCKET, level: i32, optname: i32, optval: *const u8, optlen: i32) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub fn RegOpenKeyExA(
        hKey: HKEY,
        lpSubKey: *const u8,
        ulOptions: u32,
        samDesired: u32,
        phkResult: *mut HKEY,
    ) -> i32;
    pub fn RegEnumKeyExA(
        hKey: HKEY,
        dwIndex: u32,
        lpName: *mut u8,
        lpcchName: *mut u32,
        lpReserved: *mut u32,
        lpClass: *mut u8,
        lpcchClass: *mut u32,
        lpftLastWriteTime: *mut u8,
    ) -> i32;
    pub fn RegQueryValueExA(
        hKey: HKEY,
        lpValueName: *const u8,
        lpReserved: *mut u32,
        lpType: *mut u32,
        lpData: *mut u8,
        lpcbData: *mut u32,
    ) -> i32;
    pub fn RegCloseKey(hKey: HKEY) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
}
