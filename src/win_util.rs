//! Windows helpers: finding processes and their main windows.

use anyhow::{anyhow, Result};
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible,
};

pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub title: String,
}

/// Visible top-level windows as (pid, hwnd, title).
pub fn windows() -> Vec<(u32, HWND, String)> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let out = &mut *(lp.0 as *mut Vec<(u32, HWND, String)>);
            if IsWindowVisible(hwnd).as_bool() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let len = GetWindowTextLengthW(hwnd);
                let mut buf = vec![0u16; len as usize + 1];
                let n = GetWindowTextW(hwnd, &mut buf);
                let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
                if !title.is_empty() {
                    out.push((pid, hwnd, title));
                }
            }
            BOOL(1)
        }
    }
    let mut v: Vec<(u32, HWND, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Main (first visible, titled) window of a process.
pub fn main_window(pid: u32) -> Option<HWND> {
    windows().into_iter().find(|(p, _, _)| *p == pid).map(|(_, h, _)| h)
}

pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        Some(pid)
    }
}

/// All processes that own a visible window (the ones worth capturing).
pub fn windowed_processes() -> Result<Vec<ProcInfo>> {
    let sys = System::new_with_specifics(RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()))
        .map_err(|e| anyhow!("process list failed: {e:?}"))?;
    let wins = windows();
    let mut out: Vec<ProcInfo> = Vec::new();
    for (pid, _, title) in &wins {
        if out.iter().any(|p| p.pid == *pid) {
            continue;
        }
        let Some(p) = sys.processes().get(&sysinfo::Pid::from_u32(*pid)) else { continue };
        out.push(ProcInfo {
            pid: *pid,
            name: p.name().to_string_lossy().into_owned(),
            exe: p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
            title: title.clone(),
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Find the target process: `pid:1234` / bare number, or a case-insensitive
/// substring of the exe name, exe path or window title
/// ("World of Warcraft" matches Wow.exe via its path and window title).
pub fn find_process(q: &str) -> Result<ProcInfo> {
    let ql = q.to_lowercase();
    let procs = windowed_processes()?;
    let pid_q = ql.strip_prefix("pid:").unwrap_or(&ql).parse::<u32>().ok();
    let hit = procs.into_iter().find(|p| match pid_q {
        Some(pid) => p.pid == pid,
        None => {
            p.name.to_lowercase().contains(&ql)
                || p.exe.to_lowercase().contains(&ql)
                || p.title.to_lowercase().contains(&ql)
        }
    });
    hit.ok_or_else(|| anyhow!("no running app with a window matches '{q}'. Run `sound-trigger apps` to see names."))
}
