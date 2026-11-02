// Copyright (c) 2026 leyrshroud, MIT license. https://github.com/leyrshroud/oricobridge
#![windows_subsystem = "windows"]

mod bt;
mod config;
mod ffi;
mod job;
mod logger;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bt::{Pace, Printer};
use config::Config;
use job::{GS_V0, Job};
use logger::{hex, log};

// PeriPage wake-up: resets density to 1, sent once per connection
const CMD_WAKE: [u8; 16] = [0x10, 0xff, 0xfe, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn density_cmd(level: u8) -> [u8; 5] {
    [0x10, 0xff, 0x10, 0x00, level]
}

fn print(cfg: &Config, mac: u64, data: &[u8]) -> Result<(), String> {
    let job = Job::parse(data);

    log!("job: {} bytes, {} page(s)", data.len(), job.pages.len());
    for (n, page) in job.pages.iter().enumerate() {
        log!(
            "  page {}: {}x{} bytes, driver cmds: {}",
            n + 1,
            page.row_bytes,
            page.rows.len(),
            hex(page.commands, 48)
        );
    }
    if !job.trailer.is_empty() {
        log!("  trailer: {}", hex(job.trailer, 48));
    }

    let pace = Pace {
        bytes_per_sec: cfg.rate,
        rows_per_sec: cfg.rows_per_sec,
    };
    let mut printer = Printer::connect(mac, cfg.channel, cfg.connect_attempts, pace)?;
    let start = Instant::now();

    printer.command(&CMD_WAKE, 100)?;
    printer.command(&density_cmd(cfg.density), 50)?;

    if job.pages.is_empty() {
        for chunk in data.chunks(512) {
            printer.send_paced(chunk, 0)?;
        }
        thread::sleep(Duration::from_millis(500));
        printer.feed(cfg.feed_dots)?;
    }

    for (n, page) in job.pages.iter().enumerate() {
        if cfg.forward_driver_cmds && !page.commands.is_empty() {
            printer.command(page.commands, 100)?;
        }
        printer.command(&density_cmd(cfg.density), 50)?;

        for chunk in page.rows.chunks(cfg.chunk_rows) {
            let [w0, w1] = (page.row_bytes as u16).to_le_bytes();
            let [h0, h1] = (chunk.len() as u16).to_le_bytes();
            printer.send_paced(&[GS_V0[0], GS_V0[1], GS_V0[2], GS_V0[3], w0, w1, h0, h1], 0)?;
            for row in chunk {
                printer.send_paced(row, 1)?;
            }
        }

        thread::sleep(Duration::from_millis(500));
        printer.feed(cfg.feed_dots)?;
        log!("page {} done", n + 1);
    }

    if cfg.forward_driver_cmds && !job.trailer.is_empty() {
        printer.command(job.trailer, 100)?;
    }

    log!("job done in {:.1}s", start.elapsed().as_secs_f64());
    thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn save_dump(data: &[u8]) {
    let dir = logger::exe_dir().join("jobs");
    let name = format!(
        "job_{}.bin",
        logger::timestamp().replace([' ', ':', '.'], "-")
    );
    let result = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::File::create(dir.join(&name)))
        .and_then(|mut f| f.write_all(data));

    match result {
        Ok(_) => log!("saved {name}"),
        Err(e) => log!("can't save {name}: {e}"),
    }
}

fn main() {
    let cfg = Config::load();

    log!(
        "OricoBridge {} by {}, {}",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        env!("CARGO_PKG_REPOSITORY")
    );

    unsafe {
        let mut wsa_data = [0u8; 512];
        ffi::WSAStartup(0x0202, wsa_data.as_mut_ptr());
    }

    let mac = cfg.mac.or_else(bt::find_paired_printer).unwrap_or_else(|| {
        log!(
            "printer not found among paired devices, using {}",
            bt::FALLBACK_MAC
        );
        bt::parse_mac(bt::FALLBACK_MAC).unwrap()
    });

    let listener = match TcpListener::bind(&cfg.listen) {
        Ok(l) => l,
        Err(e) => {
            log!("can't listen on {}: {e}", cfg.listen);
            return;
        }
    };

    log!(
        "started: {} -> {} ch {}, density {}, {} B/s, {} rows/s, {} rows/chunk, feed {}, forward_driver_cmds={}",
        cfg.listen,
        bt::format_mac(mac),
        if cfg.channel == 0 {
            "auto".to_string()
        } else {
            cfg.channel.to_string()
        },
        cfg.density,
        cfg.rate,
        cfg.rows_per_sec,
        cfg.chunk_rows,
        cfg.feed_dots,
        cfg.forward_driver_cmds
    );

    let cfg = Arc::new(cfg);
    let busy = Arc::new(Mutex::new(()));

    for mut conn in listener.incoming().flatten() {
        let cfg = Arc::clone(&cfg);
        let busy = Arc::clone(&busy);

        thread::spawn(move || {
            let mut data = Vec::new();
            if let Err(e) = conn.read_to_end(&mut data) {
                log!("read error after {} bytes: {e}", data.len());
            }
            drop(conn);

            if data.is_empty() {
                return;
            }
            if cfg.dump {
                save_dump(&data);
            }

            let _busy = busy.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = print(&cfg, mac, &data) {
                log!("job failed: {e}");
            }
        });
    }
}
