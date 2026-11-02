use crate::bt::parse_mac;
use crate::logger::{exe_dir, log};

#[derive(Clone)]
pub struct Config {
    pub mac: Option<u64>,
    pub listen: String,
    pub density: u8,
    pub channel: u8,
    pub rate: u64,
    pub rows_per_sec: u64,
    pub chunk_rows: usize,
    pub feed_dots: u32,
    pub forward_driver_cmds: bool,
    pub dump: bool,
    pub connect_attempts: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mac: None,
            listen: "127.0.0.1:9100".to_string(),
            density: 2,
            channel: 1,
            rate: 24_000,
            rows_per_sec: 120,
            chunk_rows: 64,
            feed_dots: 320,
            forward_driver_cmds: false,
            dump: false,
            connect_attempts: 5,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut cfg = Self::default();

        if let Ok(ini) = std::fs::read_to_string(exe_dir().join("OricoBridge.ini")) {
            for line in ini.lines().map(str::trim) {
                if line.is_empty() || line.starts_with(['#', ';', '[']) {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    cfg.set(key, value);
                }
            }
        }

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.strip_prefix("--") {
                Some(opt) => match opt.split_once('=') {
                    Some((key, value)) => cfg.set(key, value),
                    None => cfg.set(opt, &args.next().unwrap_or_default()),
                },
                None => cfg.set("mac", &arg),
            }
        }

        cfg
    }

    fn set(&mut self, key: &str, value: &str) {
        let key = key.trim().to_lowercase().replace('-', "_");
        let value = value.trim();

        let ok = match key.as_str() {
            "mac" => parse_mac(value).map(|m| self.mac = Some(m)).is_some(),
            "listen" => {
                self.listen = value.to_string();
                true
            }
            "density" => assign(&mut self.density, value, |d| d <= 2),
            "channel" if value.eq_ignore_ascii_case("auto") => {
                self.channel = 0;
                true
            }
            "channel" => assign(&mut self.channel, value, |c| (1..=30).contains(&c)),
            "rate" => assign(&mut self.rate, value, |r| r >= 1000),
            "rows_per_sec" => assign(&mut self.rows_per_sec, value, |r| r >= 10),
            "chunk_rows" => assign(&mut self.chunk_rows, value, |n| (1..=255).contains(&n)),
            "feed_dots" => assign(&mut self.feed_dots, value, |_| true),
            "connect_attempts" => assign(&mut self.connect_attempts, value, |n| n >= 1),
            "forward_driver_cmds" => parse_bool(value)
                .map(|b| self.forward_driver_cmds = b)
                .is_some(),
            "dump" => parse_bool(value).map(|b| self.dump = b).is_some(),
            _ => false,
        };

        if !ok {
            log!("config: bad setting {key}={value}, ignored");
        }
    }
}

fn assign<T: std::str::FromStr + Copy>(
    field: &mut T,
    value: &str,
    valid: impl Fn(T) -> bool,
) -> bool {
    match value.parse() {
        Ok(v) if valid(v) => {
            *field = v;
            true
        }
        _ => false,
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
