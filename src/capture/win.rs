//! Per-application audio capture via WASAPI process loopback
//! (Windows 10 build 20348+ / Windows 11; works on most 2004+ builds).
//! Without an app, captures everything playing on the default output device.

use crate::win_util;
use anyhow::{anyhow, Context, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub const CAPTURE_RATE: usize = 48_000;
const CHANNELS: usize = 2;

pub struct Capture {
    pub rx: Receiver<Vec<f32>>,
    pub source: String,
    /// PID of the captured app (None when capturing all system audio).
    pub pid: Option<i32>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn list_apps() -> Result<Vec<(String, String, i32)>> {
    Ok(win_util::windowed_processes()?
        .into_iter()
        .map(|p| (p.name, format!("{}  [{}]", p.title, p.exe), p.pid as i32))
        .collect())
}

pub fn find_pid(q: &str) -> Result<i32> {
    Ok(win_util::find_process(q)?.pid as i32)
}

fn capture_loop(pid: Option<u32>, tx: Sender<Vec<f32>>, stop: Arc<AtomicBool>, ready: Sender<Result<()>>) {
    let setup = || -> Result<(AudioClient, wasapi::AudioCaptureClient, wasapi::Handle)> {
        let _ = wasapi::initialize_mta();
        // Ask for 48 kHz float stereo; WASAPI converts from the mix format.
        let fmt = WaveFormat::new(32, 32, SampleType::Float, CAPTURE_RATE, CHANNELS, None);
        let mut client = match pid {
            Some(pid) => AudioClient::new_application_loopback_client(pid, true)
                .context("process loopback needs Windows 10 2004 or newer")?,
            None => DeviceEnumerator::new()?.get_default_device(Direction::Render)?.get_iaudioclient()?,
        };
        let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
        client.initialize_client(&fmt, Direction::Capture, mode, None)?;
        let ev = client.set_get_eventhandle()?;
        let cap = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok((client, cap, ev))
    };
    let (client, cap, ev) = match setup() {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    let frame_bytes = 4 * CHANNELS;
    let mut q: VecDeque<u8> = VecDeque::new();
    while !stop.load(Ordering::Relaxed) {
        // Events stop while the app is silent; time out so `stop` is noticed.
        let _ = ev.wait_for_event(200);
        loop {
            match cap.get_next_packet_size() {
                Ok(Some(n)) if n > 0 => {
                    if cap.read_from_device_to_deque(&mut q).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
        let frames = q.len() / frame_bytes;
        if frames == 0 {
            continue;
        }
        let bytes: Vec<u8> = q.drain(..frames * frame_bytes).collect();
        let mono: Vec<f32> = bytes
            .chunks_exact(frame_bytes)
            .map(|f| {
                let l = f32::from_le_bytes([f[0], f[1], f[2], f[3]]);
                let r = f32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                (l + r) * 0.5
            })
            .collect();
        if tx.send(mono).is_err() {
            break;
        }
    }
    let _ = client.stop_stream();
}

/// Start capturing. `app` = exe name / path / window title substring or pid;
/// None = all system audio.
pub fn start(app: Option<&str>) -> Result<Capture> {
    let (pid, source) = match app {
        Some(q) => {
            let p = win_util::find_process(q)?;
            (Some(p.pid), format!("{} ({}, pid {})", p.title, p.name, p.pid))
        }
        None => (None, "all system audio".to_string()),
    };
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("capture".into())
        .spawn(move || capture_loop(pid, tx, stop2, ready_tx))?;
    ready_rx
        .recv()
        .map_err(|_| anyhow!("capture thread exited"))?
        .context("WASAPI capture failed to start")?;
    Ok(Capture { rx, source, pid: pid.map(|p| p as i32), stop, thread: Some(thread) })
}
