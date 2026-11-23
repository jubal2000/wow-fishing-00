//! Key presses: parsing action lists and timing are shared; the actual
//! keyboard events come from the platform module (macOS / Windows).

use anyhow::{bail, Result};
use std::time::Duration;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{accessibility_ok, frontmost_pid, request_permissions};
#[cfg(target_os = "macos")]
use mac as platform;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::{accessibility_ok, frontmost_pid, request_permissions};
#[cfg(windows)]
use win as platform;

pub use platform::KeyCode;

/// Normalize key names: aliases like "comma" become ",", case-insensitive.
fn canonical(name: &str) -> String {
    let n = name.to_lowercase();
    match n.as_str() {
        "comma" => ",",
        "period" | "dot" => ".",
        "slash" => "/",
        "semicolon" => ";",
        "quote" => "'",
        "backslash" => "\\",
        "minus" => "-",
        "equal" => "=",
        "grave" | "backtick" => "`",
        "lbracket" => "[",
        "rbracket" => "]",
        "enter" => "return",
        "escape" => "esc",
        other => other,
    }
    .to_string()
}

/// Platform key code for a key name (a-z, 0-9, f1-f12, punctuation, space...).
pub fn keycode(name: &str) -> Result<KeyCode> {
    match platform::keycode(&canonical(name)) {
        Some(c) => Ok(c),
        None => bail!("unknown key '{name}'"),
    }
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

pub fn press(code: KeyCode, target: KeyTarget) -> Result<()> {
    platform::key_event(code, true, target)?;
    let (lo, hi) = (HOLD_MIN.load(Ordering::Relaxed), HOLD_MAX.load(Ordering::Relaxed));
    std::thread::sleep(Duration::from_millis(if hi > lo { rand::random_range(lo..=hi) } else { lo }));
    platform::key_event(code, false, target)
}

/// One step: wait a random delay in [min,max] ms, then press `key`.
#[derive(Clone, Debug)]
pub struct Action {
    pub key: String,
    pub code: KeyCode,
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
