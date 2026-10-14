//! Optional LibreHardwareMonitor (LHM) integration for CPU temperature.
//!
//! opendock never needs admin rights itself. If the user wants CPU sensors,
//! the settings window can install LHM (official GitHub release) into
//! `%LOCALAPPDATA%\opendock\LibreHardwareMonitor`, plus the PawnIO driver LHM
//! uses, and register a scheduled task that starts LHM elevated and minimized
//! at sign-in. Readings come from LHM's built-in web server (`data.json`) on
//! 127.0.0.1, which also works with an LHM the user runs themselves.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const PORT: u16 = 8085;
const TASK_NAME: &str = "opendock LibreHardwareMonitor";
const LHM_RELEASE_API: &str =
    "https://api.github.com/repos/LibreHardwareMonitor/LibreHardwareMonitor/releases/latest";
const LHM_ASSET: &str = "LibreHardwareMonitor.zip";
const PAWNIO_RELEASE_API: &str =
    "https://api.github.com/repos/namazso/PawnIO.Setup/releases/latest";
const PAWNIO_ASSET: &str = "PawnIO_setup.exe";
/// Marks that opendock installed PawnIO, so uninstalling only removes it then.
const PAWNIO_MARKER: &str = "pawnio-installed-by-opendock";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into());
    base.join("opendock").join("LibreHardwareMonitor")
}

fn exe_path() -> PathBuf {
    install_dir().join("LibreHardwareMonitor.exe")
}

// --- Reading -----------------------------------------------------------------

/// After a failed request, wait this long before trying again, so a missing
/// LHM costs nothing.
const RETRY_AFTER: Duration = Duration::from_secs(10);
static LAST_FAILURE: Mutex<Option<Instant>> = Mutex::new(None);

fn http_get_json(port: u16, path: &str) -> Option<Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(150)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let status = String::from_utf8_lossy(&buf[..split]);
    if !status.starts_with("HTTP/1.")
        || !status.split_whitespace().nth(1).is_some_and(|c| c == "200")
    {
        return None;
    }
    serde_json::from_slice(&buf[split + 4..]).ok()
}

/// One sensor from LHM's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Sensor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub value: f32,
}

/// First number in strings like "45.5 °C" or "45,5 °C".
fn parse_number(s: &str) -> Option<f32> {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let num: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    num.parse().ok()
}

/// Flattens LHM's `data.json` tree into its sensors.
pub fn sensors_from_json(root: &Value) -> Vec<Sensor> {
    fn walk(node: &Value, out: &mut Vec<Sensor>) {
        if let Some(id) = node.get("SensorId").and_then(Value::as_str) {
            let value = match node.get("RawValue") {
                Some(Value::Number(n)) => n.as_f64().map(|v| v as f32),
                Some(Value::String(s)) => parse_number(s),
                _ => None,
            }
            .or_else(|| {
                node.get("Value")
                    .and_then(Value::as_str)
                    .and_then(parse_number)
            });
            if let Some(value) = value {
                out.push(Sensor {
                    id: id.to_string(),
                    name: node
                        .get("Text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    kind: node
                        .get("Type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    value,
                });
            }
        }
        for child in node
            .get("Children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// The CPU package temperature among LHM's sensors.
pub fn pick_cpu_temperature(sensors: &[Sensor]) -> Option<f32> {
    let cpu_temps: Vec<&Sensor> = sensors
        .iter()
        .filter(|s| {
            (s.id.starts_with("/amdcpu") || s.id.starts_with("/intelcpu"))
                && s.id.contains("/temperature/")
        })
        .collect();
    const PREFERRED: [&str; 5] = [
        "Core (Tctl/Tdie)",
        "CPU Package",
        "Package",
        "Core Average",
        "Core Max",
    ];
    PREFERRED
        .iter()
        .find_map(|name| cpu_temps.iter().find(|s| s.name == *name))
        .or_else(|| cpu_temps.first())
        .map(|s| s.value)
}

/// All sensors, if LHM's web server answers.
pub fn read_sensors() -> Option<Vec<Sensor>> {
    if let Ok(last) = LAST_FAILURE.lock() {
        if last.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            return None;
        }
    }
    let result = http_get_json(PORT, "/data.json").map(|json| sensors_from_json(&json));
    if let Ok(mut last) = LAST_FAILURE.lock() {
        *last = result.is_none().then(Instant::now);
    }
    result
}

/// CPU package temperature in °C, if LHM provides it.
pub fn cpu_temperature() -> Option<f32> {
    pick_cpu_temperature(&read_sensors()?)
}

// --- Status --------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// Installed by opendock.
    pub installed: bool,
    /// Starts at sign-in through the scheduled task.
    pub autostart: bool,
    /// Its web server answers (any LHM, including one the user runs).
    pub running: bool,
    /// LHM provides a CPU temperature (needs the PawnIO driver).
    pub cpu_temp: bool,
}

fn hidden(cmd: &str) -> Command {
    let mut c = Command::new(cmd);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

fn task_exists() -> bool {
    hidden("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn pawnio_installed() -> bool {
    hidden("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn status() -> Status {
    // Bypass the failure back-off: the user is looking at this right now.
    let sensors = http_get_json(PORT, "/data.json").map(|j| sensors_from_json(&j));
    Status {
        installed: exe_path().exists(),
        autostart: task_exists(),
        running: sensors.is_some(),
        cpu_temp: sensors.as_deref().and_then(pick_cpu_temperature).is_some(),
    }
}

// --- Install / uninstall -------------------------------------------------------

fn latest_asset_url(api: &str, asset: &str) -> Result<String, String> {
    let out = hidden("curl")
        .args(["-sSL", "-H", "User-Agent: opendock", api])
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "could not reach GitHub: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected GitHub reply: {e}"))?;
    json.get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a.get("name").and_then(Value::as_str) == Some(asset))
        .and_then(|a| a.get("browser_download_url").and_then(Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| format!("the latest release has no {asset}"))
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let status = hidden("curl")
        .args(["-sSLf", "-H", "User-Agent: opendock", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("download failed: {url}"))
    }
}

/// LHM settings: start minimized to the tray, web server on localhost only.
fn write_lhm_settings(dir: &Path) -> Result<(), String> {
    let settings = [
        ("startMinMenuItem", "true"),
        ("minTrayMenuItem", "true"),
        ("minCloseMenuItem", "true"),
        ("runWebServerMenuItem", "true"),
        ("listenerIp", "127.0.0.1"),
        ("listenerPort", &PORT.to_string()),
    ];
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<configuration>\r\n  <appSettings>\r\n",
    );
    for (k, v) in settings {
        xml += &format!("    <add key=\"{k}\" value=\"{v}\" />\r\n");
    }
    xml += "  </appSettings>\r\n</configuration>\r\n";
    let path = dir.join("LibreHardwareMonitor.config");
    std::fs::write(&path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Task Scheduler XML: run LHM elevated at this user's sign-in, with no time
/// limit and regardless of battery power (schtasks' defaults would stop it
/// after 3 days and skip it on battery).
fn task_xml(exe: &Path) -> String {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Hardware sensors for opendock</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{exe}</Command><WorkingDirectory>{dir}</WorkingDirectory></Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(&user),
        exe = xml_escape(&exe.display().to_string()),
        dir = xml_escape(&exe.parent().unwrap_or(Path::new(".")).display().to_string()),
    )
}

/// Runs a batch script elevated (one UAC prompt) and waits for it.
fn run_elevated(script: &Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let verb = wide("runas");
    let file = wide("cmd.exe");
    let params = wide(&format!("/c \"\"{}\"\"", script.display()));
    // SAFETY: all strings are NUL-terminated and outlive the call; the process
    // handle is waited on and closed.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("administrator permission was not granted".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code == 0 {
            Ok(())
        } else {
            let log = std::fs::read_to_string(script.with_extension("log")).unwrap_or_default();
            Err(format!("setup step failed (code {code}). {}", log.trim()))
        }
    }
}

/// Downloads and sets up LHM. `progress` receives short status lines.
pub fn install(progress: &dyn Fn(&str)) -> Result<(), String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    progress("Finding the latest LibreHardwareMonitor release…");
    let url = latest_asset_url(LHM_RELEASE_API, LHM_ASSET)?;
    let zip = std::env::temp_dir().join("opendock-lhm.zip");
    progress("Downloading LibreHardwareMonitor…");
    download(&url, &zip)?;
    progress("Unpacking…");
    let tar = hidden(r"C:\Windows\System32\tar.exe")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&dir)
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    let _ = std::fs::remove_file(&zip);
    if !tar.success() || !exe_path().exists() {
        return Err("could not unpack LibreHardwareMonitor".into());
    }
    write_lhm_settings(&dir)?;

    let need_pawnio = !pawnio_installed();
    if need_pawnio {
        progress("Downloading the PawnIO driver LHM uses…");
        let url = latest_asset_url(PAWNIO_RELEASE_API, PAWNIO_ASSET)?;
        download(&url, &dir.join(PAWNIO_ASSET))?;
    }

    // Task Scheduler wants UTF-16 for XML with an encoding="UTF-16" header.
    let xml_path = dir.join("task.xml");
    let xml: Vec<u8> = std::iter::once(0xFEFFu16)
        .chain(task_xml(&exe_path()).encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    std::fs::write(&xml_path, xml).map_err(|e| format!("{}: {e}", xml_path.display()))?;

    let mut script = String::from("@echo off\r\nset LOG=\"%~dpn0.log\"\r\necho setup > %LOG%\r\n");
    if need_pawnio {
        script += &format!("\"%~dp0{PAWNIO_ASSET}\" -install -silent >> %LOG% 2>&1\r\n");
        script += &format!("if errorlevel 1 exit /b 2\r\necho.> \"%~dp0{PAWNIO_MARKER}\"\r\n");
    }
    script += &format!(
        "schtasks /Create /F /TN \"{TASK_NAME}\" /XML \"%~dp0task.xml\" >> %LOG% 2>&1\r\n"
    );
    script += "if errorlevel 1 exit /b 3\r\n";
    script += &format!("schtasks /Run /TN \"{TASK_NAME}\" >> %LOG% 2>&1\r\nexit /b 0\r\n");
    let script_path = dir.join("setup.cmd");
    std::fs::write(&script_path, script).map_err(|e| format!("{}: {e}", script_path.display()))?;

    progress("Waiting for administrator permission…");
    run_elevated(&script_path)?;
    progress("Starting LibreHardwareMonitor…");
    // Give LHM a moment to start its web server.
    for _ in 0..20 {
        if status().running {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if let Ok(mut last) = LAST_FAILURE.lock() {
        *last = None;
    }
    Ok(())
}

/// Stops and removes LHM, its start-up task, and PawnIO if opendock installed it.
pub fn uninstall(progress: &dyn Fn(&str)) -> Result<(), String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut script =
        String::from("@echo off\r\nset LOG=\"%~dpn0.log\"\r\necho uninstall > %LOG%\r\n");
    script += &format!("schtasks /End /TN \"{TASK_NAME}\" >> %LOG% 2>&1\r\n");
    script += &format!("schtasks /Delete /F /TN \"{TASK_NAME}\" >> %LOG% 2>&1\r\n");
    script += "taskkill /F /IM LibreHardwareMonitor.exe >> %LOG% 2>&1\r\n";
    if dir.join(PAWNIO_MARKER).exists() && dir.join(PAWNIO_ASSET).exists() {
        script += &format!("\"%~dp0{PAWNIO_ASSET}\" -uninstall -silent >> %LOG% 2>&1\r\n");
    }
    script += "exit /b 0\r\n";
    let script_path = dir.join("uninstall.cmd");
    std::fs::write(&script_path, script).map_err(|e| format!("{}: {e}", script_path.display()))?;
    progress("Waiting for administrator permission…");
    run_elevated(&script_path)?;
    progress("Removing files…");
    // LHM may need a moment to release its files after being stopped.
    for _ in 0..10 {
        if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "LibreHardwareMonitor was stopped, but {} could not be deleted",
        dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cpu_package_temperature() {
        let json: Value = serde_json::from_str(
            r#"{"Text":"Sensor","Children":[{"Text":"PC","Children":[
                {"Text":"AMD Ryzen 9","Children":[{"Text":"Temperatures","Children":[
                    {"Text":"Core (CCD1)","SensorId":"/amdcpu/0/temperature/3","Type":"Temperature","Value":"51,0 °C","RawValue":"51,0 °C","Children":[]},
                    {"Text":"Core (Tctl/Tdie)","SensorId":"/amdcpu/0/temperature/2","Type":"Temperature","Value":"63,4 °C","RawValue":"63,4 °C","Children":[]}
                ]}]},
                {"Text":"NVIDIA","Children":[{"Text":"GPU Core","SensorId":"/gpu-nvidia/0/temperature/0","Type":"Temperature","Value":"40.0 °C","Children":[]}]}
            ]}]}"#,
        )
        .unwrap();
        let sensors = sensors_from_json(&json);
        assert_eq!(sensors.len(), 3);
        assert_eq!(pick_cpu_temperature(&sensors), Some(63.4));
        assert_eq!(parse_number("45.5 °C"), Some(45.5));
        assert_eq!(parse_number("n/a"), None);
    }

    #[test]
    fn task_xml_is_well_formed_enough() {
        let xml = task_xml(Path::new(r"C:\a & b\LibreHardwareMonitor.exe"));
        assert!(xml.contains(r"<Command>C:\a &amp; b\LibreHardwareMonitor.exe</Command>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    }
}
