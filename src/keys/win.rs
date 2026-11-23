//! Synthetic key presses on Windows.
//! Foreground: SendInput with hardware scan codes (what games read).
//! Background: PostMessage WM_KEYDOWN/WM_KEYUP straight to the game window.

use super::KeyTarget;
use crate::win_util;
use anyhow::{anyhow, bail, Result};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC,
    MapVirtualKeyW, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_KEYDOWN, WM_KEYUP};

/// Windows virtual-key code.
pub type KeyCode = u16;

/// No special permission is needed on Windows.
pub fn accessibility_ok() -> bool {
    true
}

pub fn request_permissions() -> (bool, bool) {
    (true, true)
}

pub fn frontmost_pid() -> Option<i32> {
    win_util::foreground_pid().map(|p| p as i32)
}

/// Canonical key name -> virtual-key code (US layout).
pub fn keycode(name: &str) -> Option<KeyCode> {
    let b = name.as_bytes();
    if b.len() == 1 {
        let c = b[0];
        if c.is_ascii_lowercase() {
            return Some(c.to_ascii_uppercase() as u16); // 'A'..'Z'
        }
        if c.is_ascii_digit() {
            return Some(c as u16); // '0'..'9'
        }
    }
    let code = match name {
        "," => 0xBC,
        "." => 0xBE,
        "/" => 0xBF,
        ";" => 0xBA,
        "'" => 0xDE,
        "[" => 0xDB,
        "]" => 0xDD,
        "\\" => 0xDC,
        "-" => 0xBD,
        "=" => 0xBB,
        "`" => 0xC0,
        "space" => 0x20,
        "return" => 0x0D,
        "tab" => 0x09,
        "esc" => 0x1B,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        f if f.starts_with('f') => {
            let n: u16 = f[1..].parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        _ => return None,
    };
    Some(code)
}

fn scan_code(vk: KeyCode) -> u16 {
    unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 }
}

fn is_extended(vk: KeyCode) -> bool {
    matches!(vk, 0x25..=0x28)
}

pub fn key_event(code: KeyCode, down: bool, target: KeyTarget) -> Result<()> {
    let scan = scan_code(code);
    if target.background {
        let pid = target.pid.ok_or_else(|| anyhow!("--background needs a target app"))?;
        let hwnd = win_util::main_window(pid as u32).ok_or_else(|| anyhow!("game window not found (pid {pid})"))?;
        // lParam: repeat=1, scan code, extended flag, previous state & transition for key up.
        let mut lp: isize = 1 | ((scan as isize) << 16);
        if is_extended(code) {
            lp |= 1 << 24;
        }
        if !down {
            lp |= (1 << 30) | (1 << 31);
        }
        let msg = if down { WM_KEYDOWN } else { WM_KEYUP };
        unsafe { PostMessageW(Some(hwnd), msg, WPARAM(code as usize), LPARAM(lp)) }
            .map_err(|e| anyhow!("PostMessage failed: {e}"))?;
        return Ok(());
    }

    let mut flags = KEYEVENTF_SCANCODE;
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(code) {
        flags |= windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(code), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        bail!("SendInput was blocked (is the game running as administrator? then run this as administrator too)");
    }
    Ok(())
}
