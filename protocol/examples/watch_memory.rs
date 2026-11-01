//! Watches a memory range of a device and prints every byte that changes.
//!
//! Useful for reverse engineering unsupported devices: operate the device
//! (turn the program selector, press buttons, open the door) and observe
//! which memory addresses react. Press `Enter` at any time to insert a
//! numbered marker into the output and the log file.
//!
//! All changes are also appended to `memory_watch.csv` for later analysis.
//!
//! Usage:
//!
//! ```shell
//! cargo run --all-features --example watch_memory -- \
//!     --read-key 0809 [--full-key 5804] [--start 0000] [--end 00ff] \
//!     [--interval 250] [--learn 0] [--port /dev/ttyACM0]
//! ```
//!
//! Keys and addresses are hexadecimal. With `--learn <seconds>`, addresses
//! that change during the initial learning phase (e.g. timers) are muted.

use freemdu::{Interface, serial::Port};
use std::{
    error::Error,
    fs::OpenOptions,
    io::{BufRead, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tokio::time;

const CHUNK_SIZE: usize = 0x10;
const READ_TIMEOUT: Duration = Duration::from_millis(1000);
const ERROR_RETRY_DELAY: Duration = Duration::from_secs(4);

struct Options {
    port: String,
    read_key: u16,
    full_key: Option<u16>,
    start: u32,
    end: u32,
    interval: Duration,
    learn: Duration,
}

fn parse_hex(val: &str) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_str_radix(val.trim_start_matches("0x"), 16)?)
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut opts = Options {
        port: "/dev/ttyACM0".into(),
        read_key: 0,
        full_key: None,
        start: 0x0000,
        end: 0x00ff,
        interval: Duration::from_millis(250),
        learn: Duration::ZERO,
    };
    let mut read_key = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        let val = args.next().ok_or(format!("Missing value for {arg}"))?;

        match arg.as_str() {
            "--port" => opts.port = val,
            "--read-key" => read_key = Some(parse_hex(&val)?.try_into()?),
            "--full-key" => opts.full_key = Some(parse_hex(&val)?.try_into()?),
            "--start" => opts.start = parse_hex(&val)?,
            "--end" => opts.end = parse_hex(&val)?,
            "--interval" => opts.interval = Duration::from_millis(val.parse()?),
            "--learn" => opts.learn = Duration::from_secs(val.parse()?),
            _ => return Err(format!("Unknown argument: {arg}").into()),
        }
    }

    opts.read_key = read_key.ok_or("Missing required argument --read-key")?;

    if opts.end < opts.start {
        return Err("End address must not be lower than start address".into());
    }

    Ok(opts)
}

async fn unlock(intf: &mut Interface<Port>, opts: &Options) -> Result<(), Box<dyn Error>> {
    let id = intf.query_software_id().await?;

    intf.unlock_read_access(opts.read_key).await?;

    if let Some(key) = opts.full_key {
        intf.unlock_full_access(key).await?;
    }

    println!("Connected to device with software ID {id}");

    Ok(())
}

async fn connect(intf: &mut Interface<Port>, opts: &Options) -> Result<(), Box<dyn Error>> {
    time::timeout(READ_TIMEOUT, unlock(intf, opts)).await?
}

async fn read_range(intf: &mut Interface<Port>, opts: &Options) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut data = Vec::new();

    for addr in (opts.start..=opts.end).step_by(CHUNK_SIZE) {
        let chunk: [u8; CHUNK_SIZE] = time::timeout(READ_TIMEOUT, intf.read_memory(addr)).await??;

        data.extend_from_slice(&chunk);
    }

    data.truncate((opts.end - opts.start + 1) as usize);

    Ok(data)
}

fn print_hexdump(start: u32, data: &[u8]) {
    for (i, line) in data.chunks(0x10).enumerate() {
        let bytes: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();

        println!("{:04x}: {}", start as usize + i * 0x10, bytes.join(" "));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opts = parse_options()?;
    let port = freemdu::serial::open(&opts.port)?;
    let mut intf = Interface::new(port);
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("memory_watch.csv")?;

    // Read lines from stdin in a separate thread to insert markers
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for _ in std::io::stdin().lock().lines() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    while let Err(err) = connect(&mut intf, &opts).await {
        eprintln!("Error: {err}");
        thread::sleep(ERROR_RETRY_DELAY);
    }

    let begin = Instant::now();
    let mut prev: Option<Vec<u8>> = None;
    let mut muted = vec![false; (opts.end - opts.start + 1) as usize];
    let mut marker = 0;
    let mut learned = opts.learn.is_zero();

    writeln!(log, "# time_s,address,old,new")?;

    if !opts.learn.is_zero() {
        println!(
            "Learning noisy addresses for {:?}, do not touch the device...",
            opts.learn
        );
    }

    loop {
        let data = match read_range(&mut intf, &opts).await {
            Ok(data) => data,
            Err(err) => {
                eprintln!("Error: {err}, reconnecting...");
                thread::sleep(ERROR_RETRY_DELAY);

                if let Err(err) = connect(&mut intf, &opts).await {
                    eprintln!("Error: {err}");
                }

                continue;
            }
        };
        let elapsed = begin.elapsed();
        let learning = elapsed < opts.learn;
        let secs = elapsed.as_secs_f64();

        while rx.try_recv().is_ok() {
            marker += 1;
            println!("---------- Marker {marker} at {secs:.1} s ----------");
            writeln!(log, "# marker {marker},{secs:.3}")?;
        }

        match &prev {
            None => {
                println!("Initial memory contents:");
                print_hexdump(opts.start, &data);
            }
            Some(prev) => {
                for (i, (old, new)) in prev.iter().zip(&data).enumerate() {
                    if old == new {
                        continue;
                    }

                    let addr = opts.start as usize + i;

                    if learning {
                        muted[i] = true;
                    } else if !muted[i] {
                        println!(
                            "{secs:8.1} s  {addr:04x}: {old:02x} -> {new:02x}  ({old:3} -> {new:3})"
                        );
                    }

                    writeln!(log, "{secs:.3},{addr:04x},{old:02x},{new:02x}")?;
                }
            }
        }

        if !learned && !learning {
            learned = true;
            let addrs: Vec<String> = muted
                .iter()
                .enumerate()
                .filter(|(_, m)| **m)
                .map(|(i, _)| format!("{:04x}", opts.start as usize + i))
                .collect();

            println!("Learning finished, muted addresses: {}", addrs.join(" "));
        }

        prev = Some(data);
        time::sleep(opts.interval).await;
    }
}
