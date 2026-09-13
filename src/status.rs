use serde::Serialize;
#[cfg(not(target_os = "macos"))]
use std::sync::Mutex;

#[derive(Serialize)]
pub struct RamStatus {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f64,
}

pub fn ram() -> Option<RamStatus> {
    #[cfg(target_os = "linux")]
    {
        return linux_ram();
    }
    #[cfg(target_os = "macos")]
    {
        return macos_ram();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_ram() -> Option<RamStatus> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let val = parts.next()?.parse::<u64>().ok()?;
        match key {
            "MemTotal:" => total_kb = val,
            "MemAvailable:" => avail_kb = val,
            _ => {}
        }
    }
    if total_kb == 0 {
        return None;
    }
    let total = total_kb * 1024;
    let available = avail_kb * 1024;
    let used = total.saturating_sub(available);
    Some(RamStatus {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        used_percent: (used as f64 / total as f64) * 100.0,
    })
}

#[cfg(target_os = "macos")]
fn macos_ram() -> Option<RamStatus> {
    let total = sysctl_u64("hw.memsize")?;
    let page_size = sysctl_u64("hw.pagesize").unwrap_or(4096);
    let vm = std::process::Command::new("vm_stat").output().ok()?;
    let text = String::from_utf8(vm.stdout).ok()?;
    let mut free = 0u64;
    let mut inactive = 0u64;
    for line in text.lines() {
        if let Some(v) = parse_pages(line, "Pages free") {
            free = v;
        }
        if let Some(v) = parse_pages(line, "Pages inactive") {
            inactive = v;
        }
    }
    let available = (free + inactive).saturating_mul(page_size);
    let used = total.saturating_sub(available);
    Some(RamStatus {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        used_percent: if total == 0 {
            0.0
        } else {
            (used as f64 / total as f64) * 100.0
        },
    })
}

#[cfg(target_os = "macos")]
fn sysctl_u64(key: &str) -> Option<u64> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", key])
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()?.trim().parse().ok()
}

pub struct CpuSampler {
    #[cfg(not(target_os = "macos"))]
    last: Mutex<Option<(u64, u64)>>,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_os = "macos"))]
            last: Mutex::new(None),
        }
    }

    pub fn percent(&self) -> Option<f64> {
        #[cfg(target_os = "macos")]
        {
            return macos_cpu_percent();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let (idle, total) = cpu_times()?;
            let mut last = self.last.lock().ok()?;
            let pct = if let Some((prev_idle, prev_total)) = *last {
                let di = idle.saturating_sub(prev_idle);
                let dt = total.saturating_sub(prev_total);
                if dt == 0 {
                    None
                } else {
                    Some(((dt - di) as f64 / dt as f64) * 100.0)
                }
            } else {
                None
            };
            *last = Some((idle, total));
            pct.map(|p| p.clamp(0.0, 100.0))
        }
    }
}

#[cfg(target_os = "linux")]
fn cpu_times() -> Option<(u64, u64)> {
    linux_cpu_times()
}

#[cfg(target_os = "linux")]
fn linux_cpu_times() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().next()?;
    let mut nums = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse::<u64>().ok());
    let user = nums.next()?;
    let nice = nums.next()?;
    let system = nums.next()?;
    let idle = nums.next()?;
    let iowait = nums.next().unwrap_or(0);
    let irq = nums.next().unwrap_or(0);
    let softirq = nums.next().unwrap_or(0);
    let steal = nums.next().unwrap_or(0);
    let idle_all = idle + iowait;
    let total = user + nice + system + idle_all + irq + softirq + steal;
    Some((idle_all, total))
}

#[cfg(target_os = "macos")]
fn macos_cpu_percent() -> Option<f64> {
    let ncpu = sysctl_u64("hw.ncpu").unwrap_or(1).max(1);
    let out = std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let load: f64 = text
        .split_whitespace()
        .find_map(|s| s.trim_start_matches('{').trim_end_matches('}').parse().ok())?;
    Some(((load / ncpu as f64) * 100.0).clamp(0.0, 100.0))
}

#[cfg(target_os = "macos")]
fn parse_pages(line: &str, prefix: &str) -> Option<u64> {
    let rest = line.strip_prefix(prefix)?;
    rest.trim()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches('.')
        .trim()
        .replace(',', "")
        .parse()
        .ok()
}
