//! Synthetic key presses via CoreGraphics events.
//! Requires the Accessibility permission for the terminal running this tool.

use super::KeyTarget;
use anyhow::{anyhow, Result};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

pub type KeyCode = CGKeyCode;

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
pub fn keycode(name: &str) -> Option<KeyCode> {
    let code = match name {
        "a" => 0x00, "s" => 0x01, "d" => 0x02, "f" => 0x03, "h" => 0x04, "g" => 0x05,
        "z" => 0x06, "x" => 0x07, "c" => 0x08, "v" => 0x09, "b" => 0x0B, "q" => 0x0C,
        "w" => 0x0D, "e" => 0x0E, "r" => 0x0F, "y" => 0x10, "t" => 0x11, "1" => 0x12,
        "2" => 0x13, "3" => 0x14, "4" => 0x15, "6" => 0x16, "5" => 0x17, "=" => 0x18,
        "9" => 0x19, "7" => 0x1A, "-" => 0x1B, "8" => 0x1C, "0" => 0x1D, "]" => 0x1E,
        "o" => 0x1F, "u" => 0x20, "[" => 0x21, "i" => 0x22, "p" => 0x23, "l" => 0x25,
        "j" => 0x26, "'" => 0x27, "k" => 0x28, ";" => 0x29, "\\" => 0x2A, "," => 0x2B,
        "/" => 0x2C, "n" => 0x2D, "m" => 0x2E, "." => 0x2F, "`" => 0x32,
        "return" => 0x24, "tab" => 0x30, "space" => 0x31, "esc" => 0x35,
        "f1" => 0x7A, "f2" => 0x78, "f3" => 0x63, "f4" => 0x76, "f5" => 0x60, "f6" => 0x61,
        "f7" => 0x62, "f8" => 0x64, "f9" => 0x65, "f10" => 0x6D, "f11" => 0x67, "f12" => 0x6F,
        "left" => 0x7B, "right" => 0x7C, "down" => 0x7D, "up" => 0x7E,
        _ => return None,
    };
    Some(code)
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

pub fn key_event(code: KeyCode, down: bool, target: KeyTarget) -> Result<()> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| anyhow!("CGEventSource failed"))?;
    let ev = CGEvent::new_keyboard_event(src, code, down).map_err(|_| anyhow!("key event failed"))?;
    post(&ev, target);
    Ok(())
}
