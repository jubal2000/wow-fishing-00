//! Per-application audio capture. macOS: ScreenCaptureKit. Windows: WASAPI
//! process loopback. Both deliver 48 kHz mono f32 chunks over a channel.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;
