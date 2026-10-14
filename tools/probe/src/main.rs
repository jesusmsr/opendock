//! Hardware probe for AJAZZ Stream Dock devices.
//!
//! Draws test patterns on the keys and logs every input report, so the key
//! layout, image size and image orientation can be confirmed on real hardware.
//! Everything printed is also appended to `probe.log`.

mod pattern;

use std::fs::OpenOptions;
use std::io::Write as _;
use std::process::{exit, Command};
use std::time::{Duration, Instant};

use ajazz_hid::{Device, HidApi};
use pattern::{orient, test_pattern, to_jpeg, Orientation};

const USAGE: &str = "\
probe - AJAZZ Stream Dock hardware probe

USAGE: probe <command> [options]

COMMANDS:
  info                 List attached devices and detect the packet size
  variants             Show 15 size/orientation variants labelled A..O, then listen
  numbers              Show each key's id (1..=last) using --size/--rot/--flip, then listen
  listen               Log key events only
  brightness <0-100>   Set screen brightness
  clear                Clear all keys
  off                  Turn the screens off
  preview <dir>        Write the variant images as JPEG files (no device needed)

OPTIONS:
  --serial <s>   Use the device with this serial number
  --size <px>    Image size for `numbers` (default 85)
  --rot <deg>    Rotation for `numbers`: 0, 90, 180, 270 (default 0)
  --flip         Mirror horizontally for `numbers`
  --last <n>     Highest key id for `numbers` (default 18)
  --secs <n>     How long to listen (default 60)
  --force        Run even if the vendor Stream Dock app is open
";

/// The variant grid: label, image size and orientation per key id 1..=15.
fn variants() -> Vec<(char, u32, Orientation)> {
    let o = |rotate, flip_h| Orientation { rotate, flip_h };
    let mut v = Vec::new();
    let mut label = b'A';
    for size in [85, 96, 100] {
        for rot in [0, 90, 180, 270] {
            v.push((label as char, size, o(rot, false)));
            label += 1;
        }
    }
    v.push(('M', 85, o(0, true)));
    v.push(('N', 85, o(180, true)));
    v.push(('O', 85, o(90, true)));
    v
}

struct Opts {
    serial: Option<String>,
    size: u32,
    rot: u16,
    flip: bool,
    last: u8,
    secs: u64,
    force: bool,
    positional: Vec<String>,
}

fn parse_args() -> Opts {
    let mut o = Opts {
        serial: None,
        size: 85,
        rot: 0,
        flip: false,
        last: 18,
        secs: 60,
        force: false,
        positional: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} needs a value")))
        };
        let num = |s: String, name: &str| -> u64 {
            s.parse()
                .unwrap_or_else(|_| fail(&format!("{name}: not a number: {s}")))
        };
        match a.as_str() {
            "--serial" => o.serial = Some(val("--serial")),
            "--size" => o.size = num(val("--size"), "--size") as u32,
            "--rot" => o.rot = num(val("--rot"), "--rot") as u16,
            "--flip" => o.flip = true,
            "--last" => o.last = num(val("--last"), "--last") as u8,
            "--secs" => o.secs = num(val("--secs"), "--secs"),
            "--force" => o.force = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                exit(0);
            }
            _ if a.starts_with("--") => fail(&format!("unknown option {a}")),
            _ => o.positional.push(a),
        }
    }
    if ![0, 90, 180, 270].contains(&o.rot) {
        fail("--rot must be 0, 90, 180 or 270");
    }
    o
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}\n\n{USAGE}");
    exit(2);
}

/// Prints a line and appends it to probe.log.
fn log(line: &str) {
    println!("{line}");
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("probe.log")
    {
        let _ = writeln!(f, "{line}");
    }
}

fn vendor_app_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Stream Dock AJAZZ.exe", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Stream Dock AJAZZ"))
        .unwrap_or(false)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn printable(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
        .collect()
}

fn open(api: &HidApi, o: &Opts) -> Device {
    match Device::open(api, o.serial.as_deref()) {
        Ok(d) => {
            log(&format!(
                "opened pid={:#06x} serial={} packet={} bytes",
                d.product_id,
                d.serial,
                d.packet_size()
            ));
            d
        }
        Err(e) => {
            log(&format!("could not open device: {e}"));
            exit(1);
        }
    }
}

/// Logs input reports for `secs` seconds, sending a keep-alive every 10 s.
fn listen(dev: &Device, secs: u64) -> ajazz_hid::Result<()> {
    log(&format!(
        "listening for {secs}s - press each key once, in reading order"
    ));
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last_ping = Instant::now();
    while Instant::now() < end {
        if last_ping.elapsed() >= Duration::from_secs(10) {
            dev.keep_alive()?;
            last_ping = Instant::now();
        }
        if let Some(r) = dev.read(Duration::from_millis(200))? {
            let head = &r.raw[..r.raw.len().min(16)];
            match r.key {
                Some((key, state)) => log(&format!(
                    "key id={key:<3} state={state}   raw: {}",
                    hex(head)
                )),
                None => log(&format!("report: {}  |{}|", hex(head), printable(head))),
            }
        }
    }
    log("done listening");
    Ok(())
}

fn upload(dev: &Device, key: u8, jpeg: &[u8]) -> ajazz_hid::Result<()> {
    dev.set_key_image(key, jpeg)
}

fn run(o: &Opts) -> ajazz_hid::Result<()> {
    let cmd = o.positional.first().map(String::as_str).unwrap_or("");

    if cmd == "preview" {
        let dir = o
            .positional
            .get(1)
            .unwrap_or_else(|| fail("preview needs a directory"));
        std::fs::create_dir_all(dir).expect("create preview dir");
        for (label, size, orientation) in variants() {
            let img = orient(
                test_pattern(size, &label.to_string(), label as u32),
                orientation,
            );
            let path = format!("{dir}/{label}_{size}_{orientation}.jpg");
            std::fs::write(&path, to_jpeg(&img)).expect("write preview");
            println!("wrote {path}");
        }
        return Ok(());
    }
    if cmd.is_empty() {
        fail("missing command");
    }
    if !o.force && vendor_app_running() {
        log("The vendor 'Stream Dock AJAZZ' app is running and will fight over the device.");
        log("Quit it from the tray icon first, or pass --force.");
        exit(1);
    }

    let api = HidApi::new()?;
    match cmd {
        "info" => {
            let devs = ajazz_hid::list_devices(&api);
            if devs.is_empty() {
                log("no supported device found");
                exit(1);
            }
            for d in devs {
                log(&format!(
                    "found vid={:#06x} pid={:#06x} serial={} product={} usage_page={:#06x} path={}",
                    d.vendor_id(),
                    d.product_id(),
                    d.serial_number().unwrap_or("?"),
                    d.product_string().unwrap_or("?"),
                    d.usage_page(),
                    d.path().to_string_lossy()
                ));
            }
            open(&api, o);
        }
        "variants" => {
            let dev = open(&api, o);
            dev.set_brightness(70)?;
            dev.clear_all()?;
            for (i, (label, size, orientation)) in variants().into_iter().enumerate() {
                let img = orient(
                    test_pattern(size, &label.to_string(), i as u32),
                    orientation,
                );
                upload(&dev, i as u8 + 1, &to_jpeg(&img))?;
                log(&format!(
                    "key id {:<2} -> {label}: {size}px {orientation}",
                    i + 1
                ));
            }
            dev.flush()?;
            log("Look for the key whose letter reads upright, with the red block top-left");
            log("and the white border visible on all four sides.");
            listen(&dev, o.secs)?;
        }
        "numbers" => {
            let dev = open(&api, o);
            let orientation = Orientation {
                rotate: o.rot,
                flip_h: o.flip,
            };
            dev.set_brightness(70)?;
            dev.clear_all()?;
            for key in 1..=o.last {
                let img = orient(
                    test_pattern(o.size, &key.to_string(), key as u32),
                    orientation,
                );
                if let Err(e) = upload(&dev, key, &to_jpeg(&img)) {
                    log(&format!("key id {key}: upload failed: {e}"));
                }
            }
            dev.flush()?;
            log(&format!(
                "drew ids 1..={} at {}px {orientation}",
                o.last, o.size
            ));
            listen(&dev, o.secs)?;
        }
        "listen" => listen(&open(&api, o), o.secs)?,
        "brightness" => {
            let pct: u8 = o
                .positional
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| fail("brightness needs a value 0-100"));
            open(&api, o).set_brightness(pct)?;
        }
        "clear" => {
            let dev = open(&api, o);
            dev.clear_all()?;
            dev.flush()?;
        }
        "off" => open(&api, o).screen_off()?,
        other => fail(&format!("unknown command {other}")),
    }
    Ok(())
}

fn main() {
    let opts = parse_args();
    if let Err(e) = run(&opts) {
        log(&format!("error: {e}"));
        exit(1);
    }
}
