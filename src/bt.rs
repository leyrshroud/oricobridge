use std::ptr::null_mut;
use std::thread;
use std::time::{Duration, Instant};

use crate::ffi::{self, HKEY, SOCKET};
use crate::logger::log;

pub const FALLBACK_MAC: &str = "28:D4:1E:18:71:F7";

const BRAND_NAMES: [&str; 7] = [
    "PERIPAGE", "AIYIN", "DELI", "PIKDIK", "JADENS", "FICHERO", "SQAI",
];
const MODEL_PREFIXES: [&str; 12] = [
    "A40", "A41", "A4L", "A8", "A9", "B21", "MC220", "P40", "P80", "PD", "JD", "X8",
];
const WEAK_NAMES: [&str; 1] = ["PORTABLE"];

// Serial Port Profile UUID 00001101-0000-1000-8000-00805F9B34FB in GUID byte order
const SPP_UUID: [u8; 16] = [
    0x01, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B, 0x34, 0xFB,
];

/// "28:D4:1E:18:71:F7" (any separators) -> BTH_ADDR
pub fn parse_mac(s: &str) -> Option<u64> {
    let hex: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != 12 {
        return None;
    }
    u64::from_str_radix(&hex, 16).ok()
}

pub fn format_mac(addr: u64) -> String {
    let b = addr.to_be_bytes();
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        b[2], b[3], b[4], b[5], b[6], b[7]
    )
}

struct RegKey(HKEY);

impl RegKey {
    fn open(parent: HKEY, path: &str) -> Option<Self> {
        let path = format!("{path}\0");
        let mut key = 0;
        let rc = unsafe { ffi::RegOpenKeyExA(parent, path.as_ptr(), 0, ffi::KEY_READ, &mut key) };
        (rc == 0).then_some(Self(key))
    }

    fn subkey_name(&self, index: u32) -> Option<String> {
        let mut buf = [0u8; 256];
        let mut len = buf.len() as u32;
        let rc = unsafe {
            ffi::RegEnumKeyExA(
                self.0,
                index,
                buf.as_mut_ptr(),
                &mut len,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        (rc == 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
    }

    fn binary_value(&self, name: &str) -> Option<Vec<u8>> {
        let name = format!("{name}\0");
        let mut buf = [0u8; 256];
        let mut len = buf.len() as u32;
        let rc = unsafe {
            ffi::RegQueryValueExA(
                self.0,
                name.as_ptr(),
                null_mut(),
                null_mut(),
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        (rc == 0).then(|| buf[..len as usize].to_vec())
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe { ffi::RegCloseKey(self.0) };
    }
}

fn name_score(name: &str) -> u8 {
    let name = name.trim().to_uppercase();
    if BRAND_NAMES.iter().any(|n| name.contains(n))
        || MODEL_PREFIXES.iter().any(|n| name.starts_with(n))
    {
        2
    } else if WEAK_NAMES.iter().any(|n| name.contains(n)) {
        1
    } else {
        0
    }
}

pub fn find_paired_printer() -> Option<u64> {
    let devices = RegKey::open(
        ffi::HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices",
    )?;

    let mut candidates = Vec::new();
    let mut i = 0;
    while let Some(mac_str) = devices.subkey_name(i) {
        i += 1;
        let Some(raw) = RegKey::open(devices.0, &mac_str).and_then(|k| k.binary_value("Name"))
        else {
            continue;
        };
        let raw = raw.split(|&b| b == 0).next().unwrap_or_default();
        let name = String::from_utf8_lossy(raw).trim().to_string();

        let score = name_score(&name);
        if let (1.., Some(mac)) = (score, parse_mac(&mac_str)) {
            log!(
                "paired device \"{name}\" at {} (score {score})",
                format_mac(mac)
            );
            candidates.push((score, mac));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    if candidates.len() > 1 {
        log!("several candidates, taking the first; set mac= in OricoBridge.ini to choose");
    }
    candidates.first().map(|&(_, mac)| mac)
}

#[derive(Clone, Copy)]
pub struct Pace {
    pub bytes_per_sec: u64,
    pub rows_per_sec: u64,
}

impl Pace {
    fn cost(&self, bytes: usize, rows: u32) -> Duration {
        let by_bytes = bytes as f64 / self.bytes_per_sec as f64;
        let by_rows = rows as f64 / self.rows_per_sec as f64;
        Duration::from_secs_f64(by_bytes.max(by_rows))
    }
}

pub struct Printer {
    sock: SOCKET,
    pace: Pace,
    started: Instant,
    budget: Duration,
}

impl Printer {
    pub fn connect(mac: u64, channel: u8, attempts: u32, pace: Pace) -> Result<Self, String> {
        let addr = ffi::SOCKADDR_BTH {
            addressFamily: ffi::AF_BTH as u16,
            btAddr: mac,
            serviceClassId: if channel == 0 { SPP_UUID } else { [0; 16] },
            port: channel as u32,
        };
        let timeout_ms: u32 = 20_000;

        let mut err = 0;
        for attempt in 1..=attempts {
            unsafe {
                let s = ffi::socket(ffi::AF_BTH, ffi::SOCK_STREAM, ffi::BTHPROTO_RFCOMM);
                if s != ffi::INVALID_SOCKET {
                    let opt = &timeout_ms as *const u32 as *const u8;
                    ffi::setsockopt(s, ffi::SOL_SOCKET, ffi::SO_SNDTIMEO, opt, 4);
                    ffi::setsockopt(s, ffi::SOL_SOCKET, ffi::SO_RCVTIMEO, opt, 4);

                    if ffi::connect(s, &addr, size_of::<ffi::SOCKADDR_BTH>() as i32) == 0 {
                        log!("connected to {}", format_mac(mac));
                        return Ok(Self {
                            sock: s,
                            pace,
                            started: Instant::now(),
                            budget: Duration::ZERO,
                        });
                    }
                    err = ffi::WSAGetLastError();
                    ffi::closesocket(s);
                } else {
                    err = ffi::WSAGetLastError();
                }
            }

            log!("connect {attempt}/{attempts} failed, WSA error {err}");
            if attempt < attempts {
                thread::sleep(Duration::from_secs(attempt as u64));
            }
        }

        Err(format!(
            "can't connect to {} (WSA error {err}), is the printer on?",
            format_mac(mac)
        ))
    }

    pub fn send(&self, mut buf: &[u8]) -> Result<(), String> {
        while !buf.is_empty() {
            let n = unsafe { ffi::send(self.sock, buf.as_ptr(), buf.len() as i32, 0) };
            if n <= 0 {
                let err = unsafe { ffi::WSAGetLastError() };
                return Err(format!("send failed, WSA error {err}"));
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }

    pub fn command(&self, cmd: &[u8], pause_ms: u64) -> Result<(), String> {
        self.send(cmd)?;
        thread::sleep(Duration::from_millis(pause_ms));
        Ok(())
    }

    pub fn send_paced(&mut self, buf: &[u8], rows: u32) -> Result<(), String> {
        if self.started.elapsed() > self.budget + Duration::from_millis(50) {
            self.started = Instant::now();
            self.budget = Duration::ZERO;
        }

        self.send(buf)?;
        self.budget += self.pace.cost(buf.len(), rows);

        let ahead = self.budget.saturating_sub(self.started.elapsed());
        if !ahead.is_zero() {
            thread::sleep(ahead);
        }
        Ok(())
    }

    /// ESC J n: feed in 64-dot steps
    pub fn feed(&self, dots: u32) -> Result<(), String> {
        let mut left = dots;
        while left > 0 {
            let step = left.min(64);
            self.command(&[0x1b, 0x4a, step as u8], 80)?;
            left -= step;
        }
        Ok(())
    }
}

impl Drop for Printer {
    fn drop(&mut self) {
        unsafe { ffi::closesocket(self.sock) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_roundtrip() {
        let mac = parse_mac(FALLBACK_MAC).unwrap();
        assert_eq!(mac, 0x28D41E1871F7);
        assert_eq!(format_mac(mac), FALLBACK_MAC);
        assert_eq!(parse_mac("28d41e1871f7"), Some(mac));
        assert_eq!(parse_mac("28:D4:1E"), None);
    }

    #[test]
    fn printer_names() {
        assert_eq!(name_score("A41_71F7"), 2);
        assert_eq!(name_score("A40s"), 2);
        assert_eq!(name_score("PeriPage+DF7A"), 2);
        assert_eq!(name_score("AiYin A4L"), 2);
        assert_eq!(name_score("Deli-X8"), 2);
        assert_eq!(name_score("PD-A4"), 2);
        assert_eq!(name_score("JD-116"), 2);
        assert_eq!(name_score("P40"), 2);
        assert_eq!(name_score("Portable Printer"), 1);
        assert_eq!(name_score("Galaxy A41"), 0);
        assert_eq!(name_score("JBL Flip 5"), 0);
    }

    #[test]
    fn pace_keeps_a41_speed_and_slows_narrow_rows() {
        let pace = Pace {
            bytes_per_sec: 24_000,
            rows_per_sec: 120,
        };
        // A41, 204-byte rows: limited by bytes, same as before
        assert_eq!(pace.cost(204, 1), Duration::from_secs_f64(204.0 / 24_000.0));
        // A6, 48-byte rows: limited by rows
        assert_eq!(pace.cost(48, 1), Duration::from_secs_f64(1.0 / 120.0));
        // headers cost only their bytes
        assert_eq!(pace.cost(8, 0), Duration::from_secs_f64(8.0 / 24_000.0));
    }

    #[test]
    fn sockaddr_bth_is_30_bytes() {
        assert_eq!(size_of::<ffi::SOCKADDR_BTH>(), 30);
    }
}
