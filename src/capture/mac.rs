//! Per-application audio capture via ScreenCaptureKit (macOS 13+).
//! No virtual audio driver needed; requires the Screen Recording permission.

use anyhow::{anyhow, Context, Result};
use screencapturekit::prelude::*;
use std::sync::mpsc::{self, Receiver, Sender};

pub const CAPTURE_RATE: usize = 48_000;

struct AudioHandler {
    tx: Sender<Vec<f32>>,
}

impl SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if !matches!(of_type, SCStreamOutputType::Audio) {
            return;
        }
        let Ok(list) = sample.audio_buffer_list() else { return };
        // Non-interleaved f32: one buffer per channel. Average them to mono.
        let chans: Vec<Vec<f32>> = (&list)
            .into_iter()
            .map(|b| {
                b.data()
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect::<Vec<f32>>()
            })
            .filter(|v| !v.is_empty())
            .collect();
        let Some(first) = chans.first() else { return };
        let n = chans.iter().map(Vec::len).min().unwrap_or(0);
        let mono = if chans.len() == 1 {
            first.clone()
        } else {
            (0..n).map(|i| chans.iter().map(|c| c[i]).sum::<f32>() / chans.len() as f32).collect()
        };
        let _ = self.tx.send(mono);
    }
}

pub struct Capture {
    stream: SCStream,
    pub rx: Receiver<Vec<f32>>,
    pub source: String,
    /// PID of the captured app (None when capturing all system audio).
    pub pid: Option<i32>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

/// Match by name / bundle id substring, by PID (`pid:1234` or a bare
/// number), or by the executable path of a running process.
fn find_apps<'a>(apps: &'a [SCRunningApplication], q: &str) -> Vec<&'a SCRunningApplication> {
    let ql = q.to_lowercase();
    let pid_q = ql.strip_prefix("pid:").unwrap_or(&ql).parse::<i32>().ok();
    if let Some(pid) = pid_q {
        return apps.iter().filter(|a| a.process_id() == pid).collect();
    }
    let by_name: Vec<_> = apps
        .iter()
        .filter(|a| {
            a.application_name().to_lowercase().contains(&ql)
                || a.bundle_identifier().to_lowercase().contains(&ql)
        })
        .collect();
    if !by_name.is_empty() {
        return by_name;
    }
    // Fallback: look up PIDs whose executable path contains the query.
    let pids: Vec<i32> = std::process::Command::new("ps")
        .args(["-axo", "pid=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
        .lines()
        .filter(|l| l.to_lowercase().contains(&ql))
        .filter_map(|l| l.trim_start().split_whitespace().next()?.parse().ok())
        .collect();
    apps.iter().filter(|a| pids.contains(&a.process_id())).collect()
}

/// Resolve an app query (name / bundle id / pid / exe path) to its PID.
pub fn find_pid(q: &str) -> Result<i32> {
    let content = SCShareableContent::get().map_err(|e| anyhow!("{e:?}")).context(PERM_HINT)?;
    let apps = content.applications();
    find_apps(&apps, q)
        .first()
        .map(|a| a.process_id())
        .ok_or_else(|| anyhow!("no running app matches '{q}'"))
}

pub fn list_apps() -> Result<Vec<(String, String, i32)>> {
    let content = SCShareableContent::get().map_err(|e| anyhow!("{e:?}")).context(PERM_HINT)?;
    let mut v: Vec<_> = content
        .applications()
        .iter()
        .map(|a| (a.application_name(), a.bundle_identifier(), a.process_id()))
        .collect();
    v.sort();
    v.dedup();
    Ok(v)
}

const PERM_HINT: &str = "ScreenCaptureKit failed. Grant Screen Recording permission to your terminal app \
(System Settings > Privacy & Security > Screen & System Audio Recording), then restart the terminal.";

/// Start capturing. `app` = case-insensitive substring of the app name or
/// bundle id; None = all system audio.
pub fn start(app: Option<&str>) -> Result<Capture> {
    let content = SCShareableContent::get().map_err(|e| anyhow!("{e:?}")).context(PERM_HINT)?;
    let displays = content.displays();
    let display = displays.first().ok_or_else(|| anyhow!("no display found"))?;

    let apps = content.applications();
    let (filter, source, pid) = match app {
        Some(q) => {
            let found = find_apps(&apps, q);
            let first = found.first().ok_or_else(|| {
                let names: Vec<String> = apps
                    .iter()
                    .map(|a| format!("  {:>7}  {:?} {}", a.process_id(), a.application_name(), a.bundle_identifier()))
                    .collect();
                anyhow!(
                    "no capturable app matches '{q}'. Apps ScreenCaptureKit sees ({}):\n{}",
                    names.len(),
                    names.join("\n")
                )
            })?;
            let label = format!("{} ({}, pid {})", first.application_name(), first.bundle_identifier(), first.process_id());
            let f = SCContentFilter::create()
                .with_display(display)
                .with_including_applications(&found, &[])
                .build()
                .map_err(|e| anyhow!("{e:?}"))?;
            (f, label, Some(first.process_id()))
        }
        None => {
            let f = SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build()
                .map_err(|e| anyhow!("{e:?}"))?;
            (f, "all system audio".to_string(), None)
        }
    };

    // Video is mandatory for an SCStream; make it as cheap as possible.
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_minimum_frame_interval(&CMTime::new(1, 1))
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(CAPTURE_RATE as i32)
        .with_channel_count(1);

    let (tx, rx) = mpsc::channel();
    let mut stream = SCStream::new(&filter, &config).map_err(|e| anyhow!("{e:?}"))?;
    stream
        .add_output_handler(AudioHandler { tx }, SCStreamOutputType::Audio)
        .map_err(|e| anyhow!("{e:?}"))?;
    stream.start_capture().map_err(|e| anyhow!("{e:?}")).context(PERM_HINT)?;
    Ok(Capture { stream, rx, source, pid })
}
