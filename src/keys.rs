//! Synthetic key presses via CoreGraphics events.
//! Requires the Accessibility permission for the terminal running this tool.

use anyhow::{anyhow, bail, Result};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use std::time::Duration;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// True if this process may post synthetic key events.
pub fn accessibility_ok() -> bool {
    unsafe { CGPreflightPostEventAccess() || AXIsProcessTrusted() }
}

/// Ask macOS to show its permission prompts. This also adds the calling
/// app (your terminal) to the right list in System Settings.
pub fn request_permissions() -> (bool, bool) {
    unsafe {
        let keys = CGPreflightPostEventAccess() || CGRequestPostEventAccess();
        let screen = CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess();
        (keys, screen)
    }
}

/// macOS virtual key codes (ANSI layout).
pub fn keycode(name: &str) -> Result<CGKeyCode> {
    let n = name.to_lowercase();
    let code = match n.as_str() {
        "a" => 0x00, "s" => 0x01, "d" => 0x02, "f" => 0x03, "h" => 0x04, "g" => 0x05,
        "z" => 0x06, "x" => 0x07, "c" => 0x08, "v" => 0x09, "b" => 0x0B, "q" => 0x0C,
        "w" => 0x0D, "e" => 0x0E, "r" => 0x0F, "y" => 0x10, "t" => 0x11, "1" => 0x12,
        "2" => 0x13, "3" => 0x14, "4" => 0x15, "6" => 0x16, "5" => 0x17, "=" => 0x18,
        "9" => 0x19, "7" => 0x1A, "-" => 0x1B, "8" => 0x1C, "0" => 0x1D, "]" => 0x1E,
        "o" => 0x1F, "u" => 0x20, "[" => 0x21, "i" => 0x22, "p" => 0x23, "l" => 0x25,
        "j" => 0x26, "'" => 0x27, "k" => 0x28, ";" => 0x29, "\\" => 0x2A, "," => 0x2B,
        "/" => 0x2C, "n" => 0x2D, "m" => 0x2E, "." => 0x2F, "`" => 0x32,
        "comma" => 0x2B, "period" | "dot" => 0x2F, "slash" => 0x2C, "semicolon" => 0x29,
        "quote" => 0x27, "backslash" => 0x2A, "minus" => 0x1B, "equal" => 0x18,
        "grave" | "backtick" => 0x32, "lbracket" => 0x21, "rbracket" => 0x1E,
        "return" | "enter" => 0x24, "tab" => 0x30, "space" => 0x31, "esc" | "escape" => 0x35,
        "f1" => 0x7A, "f2" => 0x78, "f3" => 0x63, "f4" => 0x76, "f5" => 0x60, "f6" => 0x61,
        "f7" => 0x62, "f8" => 0x64, "f9" => 0x65, "f10" => 0x6D, "f11" => 0x67, "f12" => 0x6F,
        "left" => 0x7B, "right" => 0x7C, "down" => 0x7D, "up" => 0x7E,
        _ => bail!("unknown key '{name}'"),
    };
    Ok(code)
}

use std::sync::atomic::{AtomicU64, Ordering};

/// How long each key is held down, in ms (random in [min,max]).
/// Too short and a game rendering at low FPS can miss the press.
static HOLD_MIN: AtomicU64 = AtomicU64::new(80);
static HOLD_MAX: AtomicU64 = AtomicU64::new(150);

/// Parse "80-150" or "120".
pub fn set_hold(spec: &str) -> Result<()> {
    let (a, b) = spec.split_once('-').unwrap_or((spec, spec));
    let (a, b): (u64, u64) = (a.trim().parse()?, b.trim().parse()?);
    if a > b {
        bail!("bad --hold-ms range '{spec}'");
    }
    HOLD_MIN.store(a, Ordering::Relaxed);
    HOLD_MAX.store(b, Ordering::Relaxed);
    Ok(())
}

/// Where key presses go.
#[derive(Clone, Copy, Debug)]
pub struct KeyTarget {
    /// The game's process id, if known.
    pub pid: Option<i32>,
    /// true: deliver straight to `pid` even when it is in the background.
    /// false: post as normal keyboard input, but only while `pid` is frontmost.
    pub background: bool,
}

/// PID of the frontmost application (via `lsappinfo`).
pub fn frontmost_pid() -> Option<i32> {
    let run = |args: &[&str]| {
        std::process::Command::new("lsappinfo")
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let asn = run(&["front"])?;
    let info = run(&["info", "-only", "pid", &asn])?;
    // Output looks like `"pid"=12360` or `pid = 12360`.
    let after = &info[info.find("pid")? + 3..];
    let digits: String = after.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn post(ev: &CGEvent, target: KeyTarget) {
    match (target.background, target.pid) {
        (true, Some(pid)) => ev.post_to_pid(pid),
        _ => ev.post(CGEventTapLocation::HID),
    }
}

pub fn press(code: CGKeyCode, target: KeyTarget) -> Result<()> {
    let src = || CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| anyhow!("CGEventSource failed"));
    let down = CGEvent::new_keyboard_event(src()?, code, true).map_err(|_| anyhow!("key event failed"))?;
    post(&down, target);
    let (lo, hi) = (HOLD_MIN.load(Ordering::Relaxed), HOLD_MAX.load(Ordering::Relaxed));
    std::thread::sleep(Duration::from_millis(if hi > lo { rand::random_range(lo..=hi) } else { lo }));
    let up = CGEvent::new_keyboard_event(src()?, code, false).map_err(|_| anyhow!("key event failed"))?;
    post(&up, target);
    Ok(())
}

/// One step: wait a random delay in [min,max] ms, then press `key`.
#[derive(Clone, Debug)]
pub struct Action {
    pub key: String,
    pub code: CGKeyCode,
    pub min_ms: u64,
    pub max_ms: u64,
}

/// Split an action list on commas, treating a comma as the *key* when it
/// starts a step (e.g. ",:300-700,.:1500" = press ',' then '.').
fn split_steps(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        if c == ',' && !cur.trim().is_empty() && next != Some(':') {
            parts.push(std::mem::take(&mut cur));
        } else if c.is_whitespace() && cur.is_empty() {
            // skip leading spaces of a step
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

/// Parse "f:300-700,1:1500-2500" (delay optional: "f" == "f:0").
/// A comma key can be written as "," or "comma".
pub fn parse_actions(s: &str) -> Result<Vec<Action>> {
    split_steps(s)
        .iter()
        .map(|p| {
            let p = p.trim();
            let (key, delay) = p.split_once(':').unwrap_or((p, "0"));
            let (a, b) = delay.split_once('-').unwrap_or((delay, delay));
            let (min_ms, max_ms): (u64, u64) = (a.trim().parse()?, b.trim().parse()?);
            if min_ms > max_ms {
                bail!("bad delay range in '{p}'");
            }
            Ok(Action { key: key.to_string(), code: keycode(key)?, min_ms, max_ms })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys(s: &str) -> Vec<(String, u64, u64)> {
        parse_actions(s).unwrap().into_iter().map(|a| (a.key, a.min_ms, a.max_ms)).collect()
    }
    #[test]
    fn comma_key() {
        assert_eq!(keys(",:300-700,.:1500-2500"), vec![(",".into(), 300, 700), (".".into(), 1500, 2500)]);
        assert_eq!(keys("f:100,,:200"), vec![("f".into(), 100, 100), (",".into(), 200, 200)]);
        assert_eq!(keys(","), vec![(",".into(), 0, 0)]);
        assert_eq!(keys("comma:300-700, .:1500"), vec![("comma".into(), 300, 700), (".".into(), 1500, 1500)]);
        assert_eq!(keys("f:250-600"), vec![("f".into(), 250, 600)]);
        assert_eq!(keys("/:1,1:2"), vec![("/".into(), 1, 1), ("1".into(), 2, 2)]);
    }
}

/// Run the steps. Returns false if they were skipped because the game was
/// not frontmost (normal mode only — never type into another app).
pub fn run_actions(actions: &[Action], dry_run: bool, target: KeyTarget) -> Result<bool> {
    for a in actions {
        let d = if a.max_ms > a.min_ms { rand::random_range(a.min_ms..=a.max_ms) } else { a.min_ms };
        std::thread::sleep(Duration::from_millis(d));
        if !dry_run && !target.background {
            if let (Some(pid), Some(front)) = (target.pid, frontmost_pid()) {
                if pid != front {
                    println!("  !! 게임 창이 맨 앞이 아니라서 키 입력을 건너뜀 ('{}'). --background 를 쓰면 뒤에서도 동작", a.key);
                    return Ok(false);
                }
            }
        }
        let mode = if dry_run { " [dry-run]" } else if target.background { " [background]" } else { "" };
        println!("  -> press '{}' (after {d} ms){mode}", a.key);
        if !dry_run {
            press(a.code, target)?;
        }
    }
    Ok(true)
}
