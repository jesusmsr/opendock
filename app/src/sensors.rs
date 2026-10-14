//! System readings for the small screen. Everything here works without admin
//! rights: CPU usage and RAM from Win32, GPU data from NVIDIA's NVML (loaded at
//! runtime if the driver is installed). CPU temperature comes from an external
//! source (see `lhm`) because Windows does not expose it to normal programs.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

/// One sample of everything we can read. `None` = not available.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    pub cpu_usage: Option<f32>,
    pub cpu_temp: Option<f32>,
    pub ram_used_gb: Option<f32>,
    pub ram_total_gb: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub gpu_mem_used_gb: Option<f32>,
    pub gpu_mem_total_gb: Option<f32>,
}

impl Readings {
    pub fn ram_usage(&self) -> Option<f32> {
        Some(self.ram_used_gb? / self.ram_total_gb? * 100.0)
    }

    pub fn gpu_mem_usage(&self) -> Option<f32> {
        Some(self.gpu_mem_used_gb? / self.gpu_mem_total_gb? * 100.0)
    }
}

fn filetime(ft: FILETIME) -> u64 {
    (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64
}

/// Samples readings at most once per `interval`.
pub struct Sensors {
    interval: Duration,
    last: Option<Instant>,
    readings: Readings,
    /// (idle, kernel, user) from the previous sample, for CPU usage deltas.
    cpu_prev: Option<(u64, u64, u64)>,
    nvml: Option<Nvml>,
}

impl Sensors {
    pub fn new(interval: Duration) -> Self {
        Sensors {
            interval,
            last: None,
            readings: Readings::default(),
            cpu_prev: None,
            nvml: Nvml::load(),
        }
    }

    /// Current readings, refreshed if the interval has passed.
    pub fn read(&mut self) -> &Readings {
        if self.last.is_none_or(|t| t.elapsed() >= self.interval) {
            self.last = Some(Instant::now());
            self.sample();
        }
        &self.readings
    }

    fn sample(&mut self) {
        let mut r = Readings {
            cpu_usage: self.cpu_usage(),
            ..Readings::default()
        };
        // SAFETY: GlobalMemoryStatusEx fills the struct we pass, whose size is set.
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) != 0 {
                let gb = |b: u64| b as f32 / (1024.0 * 1024.0 * 1024.0);
                r.ram_total_gb = Some(gb(m.ullTotalPhys));
                r.ram_used_gb = Some(gb(m.ullTotalPhys - m.ullAvailPhys));
            }
        }
        if let Some(nvml) = &self.nvml {
            nvml.sample(&mut r);
        }
        r.cpu_temp = crate::lhm::cpu_temperature();
        self.readings = r;
    }

    fn cpu_usage(&mut self) -> Option<f32> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: GetSystemTimes writes the three FILETIMEs we pass.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        let now = (filetime(idle), filetime(kernel), filetime(user));
        let prev = self.cpu_prev.replace(now)?;
        // Kernel time includes idle time.
        let total = (now.1 - prev.1) + (now.2 - prev.2);
        let busy = total.saturating_sub(now.0 - prev.0);
        (total > 0).then(|| busy as f32 / total as f32 * 100.0)
    }
}

// --- NVML (NVIDIA Management Library), loaded dynamically from the driver ---

type NvmlDevice = *mut c_void;

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

type RawFn = unsafe extern "system" fn() -> isize;
type InitFn = unsafe extern "C" fn() -> i32;
type ByIndexFn = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type TempFn = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type UtilFn = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
type MemFn = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;

struct Nvml {
    device: NvmlDevice,
    get_temperature: TempFn,
    get_utilization: UtilFn,
    get_memory: MemFn,
}

// SAFETY: NVML handles are thread-safe according to NVIDIA's documentation.
unsafe impl Send for Nvml {}

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;

impl Nvml {
    /// Loads nvml.dll and opens the first GPU, or returns None without NVIDIA.
    fn load() -> Option<Self> {
        let name: Vec<u16> = "nvml.dll\0".encode_utf16().collect();
        // SAFETY: the library stays loaded for the process lifetime (never freed),
        // and each symbol is cast to its documented NVML signature.
        unsafe {
            let lib = LoadLibraryW(name.as_ptr());
            if lib.is_null() {
                return None;
            }
            let sym = |n: &[u8]| GetProcAddress(lib, n.as_ptr());
            let init = std::mem::transmute::<RawFn, InitFn>(sym(b"nvmlInit_v2\0")?);
            let by_index =
                std::mem::transmute::<RawFn, ByIndexFn>(sym(b"nvmlDeviceGetHandleByIndex_v2\0")?);
            if init() != NVML_SUCCESS {
                return None;
            }
            let mut device = std::ptr::null_mut();
            if by_index(0, &mut device) != NVML_SUCCESS {
                return None;
            }
            Some(Nvml {
                device,
                get_temperature: std::mem::transmute::<RawFn, TempFn>(sym(
                    b"nvmlDeviceGetTemperature\0",
                )?),
                get_utilization: std::mem::transmute::<RawFn, UtilFn>(sym(
                    b"nvmlDeviceGetUtilizationRates\0",
                )?),
                get_memory: std::mem::transmute::<RawFn, MemFn>(sym(b"nvmlDeviceGetMemoryInfo\0")?),
            })
        }
    }

    fn sample(&self, r: &mut Readings) {
        // SAFETY: valid device handle and out-pointers to correctly laid out structs.
        unsafe {
            let mut t = 0u32;
            if (self.get_temperature)(self.device, NVML_TEMPERATURE_GPU, &mut t) == NVML_SUCCESS {
                r.gpu_temp = Some(t as f32);
            }
            let mut u = NvmlUtilization { gpu: 0, memory: 0 };
            if (self.get_utilization)(self.device, &mut u) == NVML_SUCCESS {
                r.gpu_usage = Some(u.gpu as f32);
            }
            let mut m = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            if (self.get_memory)(self.device, &mut m) == NVML_SUCCESS {
                let gb = |b: u64| b as f32 / (1024.0 * 1024.0 * 1024.0);
                r.gpu_mem_total_gb = Some(gb(m.total));
                r.gpu_mem_used_gb = Some(gb(m.used));
            }
        }
    }
}
