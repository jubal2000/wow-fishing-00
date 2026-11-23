mod capture;
mod features;
mod keys;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use features::{Detector, Event, Frame, Template};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(about = "Detect a specific sound in a running app's audio and press keys in response")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// How long each key is held down in ms, "min-max" or a single value.
    #[arg(long, global = true, default_value = "80-150")]
    hold_ms: String,
}

/// Detection tuning, shared by `test` and `run`.
#[derive(clap::Args, Clone)]
struct DetectOpts {
    /// Reference sound(s). Repeat or comma-separate to use several; the best
    /// match wins (e.g. the original clip + splashes recorded in-game).
    #[arg(long, value_delimiter = ',', default_value = "template.wav")]
    template: Vec<PathBuf>,
    /// Similarity needed to trigger, 0..1. Lower = catches more, more false triggers.
    #[arg(long, default_value_t = 0.75)]
    threshold: f32,
    /// Report peaks between this and --threshold as "near miss" so you can see
    /// what was missed. Default: threshold - 0.1. In `run` these are shown only
    /// with -v or when --near is given.
    #[arg(long)]
    near: Option<f32>,
    /// Ignore audio quieter than this (dBFS). Raise (e.g. -55) to ignore faint
    /// far-away sounds, lower to catch quieter ones.
    #[arg(long, default_value_t = -70.0, allow_negative_numbers = true)]
    min_db: f32,
    /// How long (ms) the score must stop rising before a peak is judged.
    /// Smaller = faster reaction, larger = more accurate peak.
    #[arg(long, default_value_t = 60)]
    peak_ms: u64,
}

impl DetectOpts {
    fn build(&self) -> Result<Detector> {
        anyhow::ensure!((0.0..=1.0).contains(&self.threshold), "--threshold must be between 0 and 1");
        // A directory means "every .wav/.ogg inside it".
        let mut paths = Vec::new();
        for p in &self.template {
            if p.is_dir() {
                let mut v: Vec<PathBuf> = std::fs::read_dir(p)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|f| {
                        f.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav") || x.eq_ignore_ascii_case("ogg"))
                    })
                    .collect();
                v.sort();
                anyhow::ensure!(!v.is_empty(), "no .wav/.ogg files in {}", p.display());
                paths.extend(v);
            } else {
                paths.push(p.clone());
            }
        }
        let mut ts = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let t = load_template(p)?;
            println!("Template #{}: {} ({:.0} ms, {:.1} dBFS)", i + 1, p.display(), t.frames as f32 * 10.0, t.db);
            ts.push(t);
        }
        let mut d = Detector::new(ts, self.threshold, self.min_db);
        d.near = self.near.unwrap_or(self.threshold - 0.1).max(0.0);
        d.peak_hold = ((self.peak_ms as f32 / 10.0).round() as usize).max(1);
        println!(
            "Detect: threshold {:.2}, near-miss >= {:.2}, min {:.0} dBFS, peak {} ms",
            d.threshold, d.near, d.min_db, d.peak_hold * 10
        );
        Ok(d)
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Check / request the macOS permissions (key events + audio capture).
    Perms,
    /// List running applications you can capture audio from.
    Apps,
    /// Press keys after a countdown — to check that the game accepts them.
    /// e.g. `press "," --delay 5`
    Press {
        /// Same syntax as --actions, e.g. "," or ",:0,,:500" (press twice).
        actions: String,
        #[arg(long, default_value_t = 3.0)]
        delay: f32,
        /// Target app (needed for --background).
        #[arg(long)]
        app: Option<String>,
        /// Send keys straight to the app even when it is in the background.
        #[arg(long)]
        background: bool,
    },
    /// Record an app's audio to a WAV file (use this to capture the target sound).
    Record {
        /// App name or bundle id substring (omit = all system audio).
        #[arg(long)]
        app: Option<String>,
        #[arg(long, default_value_t = 10.0)]
        secs: f32,
        #[arg(long, default_value = "recording.wav")]
        out: PathBuf,
    },
    /// Cut the target sound out of a recording to make a template.
    /// Without --start/--end, auto-picks the loudest event.
    Trim {
        input: PathBuf,
        #[arg(long, default_value = "template.wav")]
        out: PathBuf,
        /// Start time in seconds.
        #[arg(long)]
        start: Option<f32>,
        /// End time in seconds.
        #[arg(long)]
        end: Option<f32>,
        /// Auto mode: max template length in seconds.
        #[arg(long, default_value_t = 1.2)]
        max_secs: f32,
    },
    /// Run the detector over a WAV file offline and print the hits (for tuning).
    Test {
        input: PathBuf,
        #[command(flatten)]
        det: DetectOpts,
    },
    /// Listen live and press keys when the sound is detected.
    Run {
        #[arg(long)]
        app: Option<String>,
        #[command(flatten)]
        det: DetectOpts,
        /// Keys to press on detection: "key:minms-maxms,...", e.g. "f:300-700".
        #[arg(long, default_value = "f:250-600")]
        actions: String,
        /// Seconds to wait before starting (time to switch to the game window).
        #[arg(long, default_value_t = 3.0)]
        start_delay: f32,
        /// Keys to press once at startup (e.g. cast), same syntax.
        #[arg(long)]
        start_actions: Option<String>,
        /// If nothing is detected for this many seconds, run --timeout-actions.
        #[arg(long)]
        timeout: Option<f32>,
        #[arg(long)]
        timeout_actions: Option<String>,
        /// Ignore audio for this long after pressing keys (seconds).
        #[arg(long, default_value_t = 1.5)]
        cooldown: f32,
        /// Print the matches but don't press keys.
        #[arg(long)]
        dry_run: bool,
        /// Show a live similarity meter.
        #[arg(long, short)]
        verbose: bool,
        /// Send keys straight to the game even while you use other apps.
        /// Without it, keys are only pressed while the game is frontmost.
        #[arg(long)]
        background: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    keys::set_hold(&cli.hold_ms)?;
    match cli.cmd {
        Cmd::Press { actions, delay, app, background } => {
            let actions = keys::parse_actions(&actions)?;
            if !keys::accessibility_ok() {
                bail!("Accessibility permission is missing — run `sound-trigger perms`.");
            }
            if background && app.is_none() {
                bail!("--background needs --app");
            }
            let pid = app.as_deref().map(capture::find_pid).transpose()?;
            if let Some(p) = pid {
                println!("Target pid {p}{}", if background { " (background)" } else { "" });
            }
            countdown(delay);
            keys::run_actions(&actions, false, keys::KeyTarget { pid, background })?;
        }
        Cmd::Perms => {
            let (keys_ok, screen_ok) = keys::request_permissions();
            let mark = |b: bool| if b { "OK" } else { "필요" };
            println!("키 입력 권한 (Accessibility / 기기 제어): {}", mark(keys_ok));
            println!("화면 및 시스템 오디오 녹음:              {}", mark(screen_ok));
            if !(keys_ok && screen_ok) {
                println!(
                    "\n시스템 설정에서 방금 추가된 터미널 앱의 스위치를 켠 뒤,\n\
                     터미널을 완전히 종료(Cmd+Q)하고 다시 열어 `sound-trigger perms`로 확인하세요."
                );
            }
        }
        Cmd::Apps => {
            for (name, bundle, pid) in capture::list_apps()? {
                println!("{pid:>7}  {name:<32} {bundle}");
            }
        }
        Cmd::Record { app, secs, out } => record(app.as_deref(), secs, &out)?,
        Cmd::Trim { input, out, start, end, max_secs } => trim(&input, &out, start, end, max_secs)?,
        Cmd::Test { input, det } => test(&input, &det)?,
        Cmd::Run {
            app, det, actions, start_delay, start_actions, timeout, timeout_actions,
            cooldown, dry_run, verbose, background,
        } => {
            let actions = keys::parse_actions(&actions)?;
            let start_actions = start_actions.as_deref().map(keys::parse_actions).transpose()?;
            let timeout_actions = timeout_actions.as_deref().map(keys::parse_actions).transpose()?;
            if timeout.is_some() != timeout_actions.is_some() {
                bail!("--timeout and --timeout-actions must be used together");
            }
            run(RunOpts {
                app, det, actions, start_delay, start_actions,
                timeout: timeout.zip(timeout_actions), cooldown, dry_run, verbose, background,
            })?
        }
    }
    Ok(())
}

// ---------- WAV helpers ----------

/// Load a WAV or Ogg Vorbis file as mono f32 + sample rate.
fn read_wav(path: &Path) -> Result<(Vec<f32>, usize)> {
    let is_ogg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ogg"));
    if is_ogg {
        return read_ogg(path);
    }
    let mut r = hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
        }
    };
    let mono = raw.chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect();
    Ok((mono, spec.sample_rate as usize))
}

fn read_ogg(path: &Path) -> Result<(Vec<f32>, usize)> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut r = lewton::inside_ogg::OggStreamReader::new(f).context("not an Ogg Vorbis file")?;
    let ch = r.ident_hdr.audio_channels as usize;
    let rate = r.ident_hdr.audio_sample_rate as usize;
    let mut mono = Vec::new();
    while let Some(pkt) = r.read_dec_packet_itl()? {
        mono.extend(pkt.chunks(ch).map(|c| c.iter().map(|&v| v as f32).sum::<f32>() / (ch as f32 * 32768.0)));
    }
    Ok((mono, rate))
}

fn write_wav(path: &Path, samples: &[f32], rate: usize) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample(s)?;
    }
    w.finalize()?;
    Ok(())
}

fn load_frames(path: &Path) -> Result<Vec<Frame>> {
    let (s, rate) = read_wav(path)?;
    Ok(features::extract_all(&features::resample_to_16k(&s, rate)))
}

// ---------- commands ----------

fn record(app: Option<&str>, secs: f32, out: &Path) -> Result<()> {
    let cap = capture::start(app)?;
    println!("Recording {secs:.1}s from {} ... make the sound happen now.", cap.source);
    let mut buf = Vec::new();
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < end {
        if let Ok(chunk) = cap.rx.recv_timeout(Duration::from_millis(100)) {
            buf.extend(chunk);
        }
    }
    drop(cap);
    if buf.is_empty() {
        bail!("no audio received — is the app producing sound, and is Screen Recording permission granted?");
    }
    let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
    write_wav(out, &buf, capture::CAPTURE_RATE)?;
    println!(
        "Saved {} ({:.1}s, peak {:.1} dBFS).",
        out.display(),
        buf.len() as f32 / capture::CAPTURE_RATE as f32,
        20.0 * (peak + 1e-9).log10()
    );
    Ok(())
}

fn trim(input: &Path, out: &Path, start: Option<f32>, end: Option<f32>, max_secs: f32) -> Result<()> {
    let (s, rate) = read_wav(input)?;
    let s16 = features::resample_to_16k(&s, rate);
    let (a, b) = match (start, end) {
        (Some(a), Some(b)) => ((a * 16000.0) as usize, (b * 16000.0) as usize),
        (None, None) => {
            let frames = features::extract_all(&s16);
            features::auto_trim(&frames, max_secs, 25.0).context("recording is empty")?
        }
        _ => bail!("give both --start and --end, or neither"),
    };
    let b = b.min(s16.len());
    if a >= b {
        bail!("empty selection");
    }
    let clip = &s16[a..b];
    Template::from_frames(&features::extract_all(clip))?; // validate
    write_wav(out, clip, features::SR)?;
    println!(
        "Template {} = {:.2}s .. {:.2}s ({:.0} ms). Listen to it to confirm it's the right sound:\n  afplay {}",
        out.display(),
        a as f32 / 16000.0,
        b as f32 / 16000.0,
        (b - a) as f32 / 16.0,
        out.display()
    );
    Ok(())
}

fn load_template(path: &Path) -> Result<Template> {
    Template::from_frames(&load_frames(path)?)
}

fn test(input: &Path, o: &DetectOpts) -> Result<()> {
    let mut det = o.build()?;
    let mut best = (0f32, 0f32);
    let mut hits = 0;
    for (i, f) in load_frames(input)?.into_iter().enumerate() {
        let tsec = i as f32 / features::FPS;
        match det.feed(f) {
            Some(Event::Hit { score, template }) => {
                hits += 1;
                println!("  HIT       {tsec:7.2}s  score {score:.3}  (template #{})", template + 1);
            }
            Some(Event::NearMiss { score, template }) => {
                println!("  near miss {tsec:7.2}s  score {score:.3}  (template #{})", template + 1);
            }
            None => {}
        }
        if det.last_score > best.0 {
            best = (det.last_score, tsec);
        }
    }
    println!("{hits} hit(s). Best score {:.3} at {:.2}s (threshold {}).", best.0, best.1, o.threshold);
    Ok(())
}

struct RunOpts {
    app: Option<String>,
    det: DetectOpts,
    actions: Vec<keys::Action>,
    start_delay: f32,
    start_actions: Option<Vec<keys::Action>>,
    timeout: Option<(f32, Vec<keys::Action>)>,
    cooldown: f32,
    dry_run: bool,
    verbose: bool,
    background: bool,
}

fn run(o: RunOpts) -> Result<()> {
    if !o.dry_run && !keys::accessibility_ok() {
        bail!(
            "Accessibility permission is missing, key presses would be ignored.\n\
             System Settings > Privacy & Security > Accessibility: enable your terminal app, then restart it.\n\
             (Or use --dry-run to test detection only.)"
        );
    }
    let mut det = o.det.build()?;
    let cap = capture::start(o.app.as_deref())?;
    println!("Listening to {}. Ctrl+C to stop.", cap.source);
    if o.background {
        anyhow::ensure!(cap.pid.is_some(), "--background needs --app");
        println!("Background mode: keys go straight to the game (pid {}), you can use other apps.", cap.pid.unwrap());
    } else {
        println!("Keys are pressed only while the game window is frontmost (add --background to allow other apps).");
    }
    let target = keys::KeyTarget { pid: cap.pid, background: o.background };

    let mut rs = features::Resampler::new(capture::CAPTURE_RATE);
    let mut ex = features::Extractor::new();
    let (mut s16, mut frames) = (Vec::new(), Vec::new());
    let mut ignore_until = Instant::now();
    let mut last_event = Instant::now();
    let mut last_meter = Instant::now();
    let mut peak_score = 0f32;
    // Detect a muted game (e.g. WoW silences itself in the background).
    let mut silent_since: Option<Instant> = None;
    let mut warned_silent = false;

    // After pressing keys: drop buffered audio and pause detection briefly so
    // our own action's sounds don't retrigger.
    let after_actions = |det: &mut Detector, cap: &capture::Capture| {
        while cap.rx.try_recv().is_ok() {}
        det.reset();
        Instant::now() + Duration::from_secs_f32(o.cooldown)
    };

    if o.start_delay > 0.0 {
        countdown(o.start_delay);
        while cap.rx.try_recv().is_ok() {}
        last_event = Instant::now();
    }

    if let Some(sa) = &o.start_actions {
        println!("Start actions:");
        keys::run_actions(sa, o.dry_run, target)?;
        ignore_until = after_actions(&mut det, &cap);
        last_event = Instant::now();
    }

    loop {
        let chunk = match cap.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Vec::new(),
            Err(e) => bail!("capture stopped: {e}"),
        };

        if !chunk.is_empty() {
            if chunk.iter().all(|&v| v == 0.0) {
                let since = *silent_since.get_or_insert_with(Instant::now);
                if !warned_silent && since.elapsed() > Duration::from_secs(5) {
                    if o.verbose {
                        println!();
                    }
                    println!(
                        "[{}] !! 게임 소리가 완전히 0입니다. WoW가 뒤에 있을 때 소리를 끄는 중일 수 있습니다.\n\
                         \x20  WoW 채팅창: /console Sound_EnableSoundWhenGameIsInBG 1\n\
                         \x20  (또는 설정 > 소리 > '배경에서 소리 재생' 켜기)",
                        now()
                    );
                    warned_silent = true;
                }
            } else {
                silent_since = None;
                warned_silent = false;
            }
        }

        if let Some((secs, ta)) = &o.timeout {
            if last_event.elapsed().as_secs_f32() > *secs {
                println!("[{}] no detection for {secs:.0}s -> timeout actions", now());
                keys::run_actions(ta, o.dry_run, target)?;
                ignore_until = after_actions(&mut det, &cap);
                last_event = Instant::now();
                continue;
            }
        }
        if Instant::now() < ignore_until {
            continue;
        }

        s16.clear();
        frames.clear();
        rs.process(&chunk, &mut s16);
        ex.push(&s16, &mut frames);
        let mut fired = None;
        for f in frames.drain(..) {
            peak_score = peak_score.max(det.last_score);
            match det.feed(f) {
                Some(Event::Hit { score, template }) => {
                    fired = Some((score, template));
                    break;
                }
                Some(Event::NearMiss { score, template }) if o.verbose || o.det.near.is_some() => {
                    if o.verbose {
                        println!();
                    }
                    println!(
                        "[{}] near miss (score {score:.3} < threshold {:.2}, template #{})",
                        now(),
                        o.det.threshold,
                        template + 1
                    );
                }
                _ => {}
            }
        }

        if let Some((score, template)) = fired {
            if o.verbose {
                println!();
            }
            let tn = if det.templates.len() > 1 { format!(", template #{}", template + 1) } else { String::new() };
            println!("[{}] DETECTED (score {score:.3}{tn})", now());
            keys::run_actions(&o.actions, o.dry_run, target)?;
            ignore_until = after_actions(&mut det, &cap);
            last_event = Instant::now();
        } else if o.verbose && last_meter.elapsed() > Duration::from_millis(100) {
            let n = (peak_score.clamp(0.0, 1.0) * 40.0) as usize;
            let tmark = (o.det.threshold * 40.0) as usize;
            let bar: String = (0..40).map(|i| if i < n { '#' } else if i == tmark { '|' } else { '.' }).collect();
            print!("\r  score {peak_score:.3} [{bar}] {:6.1} dB ", det.last_db);
            let _ = std::io::stdout().flush();
            peak_score = 0.0;
            last_meter = Instant::now();
        }
    }
}

fn countdown(secs: f32) {
    let mut left = secs;
    while left > 0.0 {
        print!("\r시작까지 {left:.0}초... 게임 창을 앞으로 띄우세요. ");
        let _ = std::io::stdout().flush();
        let step = left.min(1.0);
        std::thread::sleep(Duration::from_secs_f32(step));
        left -= step;
    }
    println!("\r시작!                                        ");
}

fn now() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let s = d.as_secs() % 86400;
    format!("{:02}:{:02}:{:02}", (s / 3600 + 9) % 24, s / 60 % 60, s % 60) // KST
}
