use std::collections::HashSet;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::editor::{self, AudioTrackMeta};
use crate::settings::{self, AppSettings};

type Error = Box<dyn std::error::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    pub replay_buffer_active: bool,
    pub recording_active: bool,
}

pub struct Recorder {
    replay: Option<Child>,
    recording: Option<Child>,
    replay_tracks: Vec<AudioTrackMeta>,
    recording_tracks: Vec<AudioTrackMeta>,
    recording_output: Option<String>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            replay: None,
            recording: None,
            replay_tracks: Vec::new(),
            recording_tracks: Vec::new(),
            recording_output: None,
        }
    }

    pub fn get_state(&mut self) -> RecordingState {
        clear_exited_child(&mut self.replay);
        clear_exited_child(&mut self.recording);
        RecordingState {
            replay_buffer_active: self.replay.is_some(),
            recording_active: self.recording.is_some(),
        }
    }

    pub fn start_replay_buffer(&mut self, s: &AppSettings) -> Result<(), Error> {
        if self.replay.is_some() {
            return Err("replay buffer already running".into());
        }

        let tracks = audio_tracks(s);
        let mut cmd = base_command(s, &tracks);
        let output = settings::resolve_user_path(&s.clips_directory);
        cmd.args(["-r", &s.buffer_seconds.to_string()])
            .args(["-c", &s.container])
            .args(["-o", &output.to_string_lossy()]);

        let mut child = cmd.spawn()?;
        ensure_started(&mut child, "replay buffer")?;
        self.replay = Some(child);
        self.replay_tracks = tracks;
        Ok(())
    }

    pub fn stop_replay_buffer(&mut self) -> Result<(), Error> {
        let mut child = self.replay.take().ok_or("no replay buffer running")?;
        send_signal(&child, libc::SIGINT)?;
        child.wait()?;
        self.replay_tracks.clear();
        Ok(())
    }

    pub fn save_replay(&mut self) -> Result<(), Error> {
        clear_exited_child(&mut self.replay);
        let child = self.replay.as_ref().ok_or("replay buffer is not running")?;
        send_signal(child, libc::SIGUSR1)
    }

    pub fn start_recording(&mut self, s: &AppSettings) -> Result<(), Error> {
        if self.recording.is_some() {
            return Err("already recording".into());
        }

        let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_dir = settings::resolve_user_path(&s.clips_directory);
        let output = output_dir.join(format!("{}.{}", stamp, s.container));

        let tracks = audio_tracks(s);
        let mut cmd = base_command(s, &tracks);
        cmd.args(["-o", &output.to_string_lossy()]);

        let mut child = cmd.spawn()?;
        ensure_started(&mut child, "recording")?;
        self.recording = Some(child);
        self.recording_output = Some(output.to_string_lossy().into_owned());
        self.recording_tracks = tracks.clone();
        editor::write_audio_tracks_sidecar(
            self.recording_output.as_deref().unwrap_or_default(),
            &tracks,
        )
        .ok();
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<String, Error> {
        let mut child = self.recording.take().ok_or("not recording")?;
        send_signal(&child, libc::SIGINT)?;
        child.wait()?;
        self.recording_output = None;
        self.recording_tracks.clear();
        Ok("recording saved".into())
    }

    pub fn replay_tracks(&self) -> Vec<AudioTrackMeta> {
        self.replay_tracks.clone()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        for child in [self.replay.as_mut(), self.recording.as_mut()]
            .into_iter()
            .flatten()
        {
            send_signal(child, libc::SIGINT).ok();
            child.wait().ok();
        }
    }
}

fn base_command(s: &AppSettings, tracks: &[AudioTrackMeta]) -> Command {
    let mut cmd = Command::new("gpu-screen-recorder");
    let capture_target = capture_target();
    cmd.args(["-w", &capture_target])
        .args(["-f", &s.fps.to_string()])
        .args(["-k", &s.codec])
        .args(["-ac", &s.audio_codec]);

    if capture_target == "portal" {
        cmd.args(["-restore-portal-session", "yes"]);
    }

    if tracks.is_empty() {
        let audio_source = audio_source_fallback(s);
        if !audio_source.is_empty() {
            cmd.args(["-a", &audio_source]);
        }
    } else {
        for track in tracks {
            cmd.args(["-a", &track.source]);
        }
    }
    cmd
}

fn audio_tracks(s: &AppSettings) -> Vec<AudioTrackMeta> {
    let mut tracks = Vec::new();
    let apps = available_application_audio();
    let wants_app_tracks = s.audio_discord || s.audio_spotify;

    if s.audio_focused_window {
        tracks.push(AudioTrackMeta {
            key: "desktop".into(),
            label: if wants_app_tracks {
                "desktop_mix".into()
            } else {
                "desktop".into()
            },
            source: "default_output".into(),
            isolated: !wants_app_tracks,
        });
    }

    if s.audio_discord {
        for app in ["vesktop", "discord"] {
            if has_application_audio(&apps, app) {
                tracks.push(AudioTrackMeta {
                    key: format!("app_{app}"),
                    label: app.into(),
                    source: format!("app:{app}"),
                    isolated: true,
                });
            }
        }
    }
    if s.audio_spotify && has_application_audio(&apps, "spotify") {
        tracks.push(AudioTrackMeta {
            key: "spotify".into(),
            label: "spotify".into(),
            source: "app:spotify".into(),
            isolated: true,
        });
    }
    if s.audio_microphone {
        let mic_source = s.audio_microphone_source.trim();
        tracks.push(AudioTrackMeta {
            key: "microphone".into(),
            label: "microphone".into(),
            source: if mic_source.is_empty() {
                "default_input".into()
            } else {
                mic_source.to_string()
            },
            isolated: true,
        });
    }

    if tracks.is_empty() && !s.audio_source.trim().is_empty() {
        tracks.push(AudioTrackMeta {
            key: "audio".into(),
            label: "audio".into(),
            source: s.audio_source.clone(),
            isolated: !s.audio_source.contains('|'),
        });
    }

    tracks
}

fn audio_source_fallback(s: &AppSettings) -> String {
    s.audio_source.clone()
}

fn capture_target() -> String {
    if std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("hyprland")
    {
        if let Some(monitor) = active_hyprland_monitor() {
            return monitor;
        }
    }
    "portal".into()
}

fn active_hyprland_monitor() -> Option<String> {
    let output = Command::new("hyprctl")
        .args(["activeworkspace", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()?
        .get("monitor")?
        .as_str()
        .filter(|monitor| !monitor.is_empty())
        .map(str::to_owned)
}

fn available_application_audio() -> HashSet<String> {
    Command::new("gpu-screen-recorder")
        .arg("--list-application-audio")
        .output()
        .ok()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|line| line.trim().to_lowercase())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn has_application_audio(apps: &HashSet<String>, name: &str) -> bool {
    let needle = name.to_lowercase();
    apps.contains(&needle)
}

fn ensure_started(child: &mut Child, name: &str) -> Result<(), Error> {
    thread::sleep(Duration::from_millis(250));
    if let Some(status) = child.try_wait()? {
        return Err(format!("{name} exited during startup ({status})").into());
    }
    Ok(())
}

fn clear_exited_child(child: &mut Option<Child>) {
    let exited = child
        .as_mut()
        .and_then(|process| process.try_wait().ok())
        .flatten()
        .is_some();
    if exited {
        child.take();
    }
}

fn send_signal(child: &Child, signal: i32) -> Result<(), Error> {
    let result = unsafe { libc::kill(child.id() as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}
