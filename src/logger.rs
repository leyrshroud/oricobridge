use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::ffi;

const MAX_LOG_SIZE: u64 = 1024 * 1024;

macro_rules! log {
    ($($arg:tt)*) => { $crate::logger::write(&format!($($arg)*)) };
}
pub(crate) use log;

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}

fn log_file() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let path = exe_dir().join("OricoBridge.log");
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(_) => path,
            Err(_) => std::env::temp_dir().join("OricoBridge.log"),
        }
    })
}

pub fn timestamp() -> String {
    let mut t = ffi::SYSTEMTIME::default();
    unsafe { ffi::GetLocalTime(&mut t) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

pub fn write(msg: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _lock = LOCK.lock();

    let path = log_file();
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_SIZE) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {}", timestamp(), msg);
    }
}

pub fn hex(bytes: &[u8], limit: usize) -> String {
    let mut s: String = bytes
        .iter()
        .take(limit)
        .map(|b| format!("{b:02x}"))
        .collect();
    if bytes.len() > limit {
        s += "...";
    }
    s
}
