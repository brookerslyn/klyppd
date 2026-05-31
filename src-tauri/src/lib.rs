mod db;
mod editor;
mod r2;
mod recorder;
mod settings;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use chrono::{Duration, Local, Utc};
use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, Manager};

use db::{Clip, Database};
use recorder::{Recorder, RecordingState};
use settings::AppSettings;

const PENDING_NAME_PATH: &str = "/tmp/klyppd-pending-name";
const PENDING_AUDIO_TRACKS_PATH: &str = "/tmp/klyppd-pending-audio-tracks";
const PREVIEW_DIR: &str = "klyppd-preview";

// TODO: configurable preview cache limit (currently grows unbounded)

pub struct AppState {
    pub db: Mutex<Database>,
    pub recorder: Mutex<Recorder>,
    pub settings: Mutex<AppSettings>,
}

#[derive(Debug, Clone, Serialize)]
struct AudioDeviceOption {
    value: String,
    label: String,
}

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

// Library queries -------------------------------------------------------------

#[tauri::command]
fn get_clips(state: tauri::State<AppState>) -> Result<Vec<Clip>, String> {
    state.db.lock().unwrap().get_all_clips().map_err(err)
}

#[tauri::command]
fn get_clips_by_folder(state: tauri::State<AppState>, folder: String) -> Result<Vec<Clip>, String> {
    state.db.lock().unwrap().get_clips_by_folder(&folder).map_err(err)
}

#[tauri::command]
fn get_uploaded_clips(state: tauri::State<AppState>, permanent: bool) -> Result<Vec<Clip>, String> {
    state.db.lock().unwrap().get_uploaded_clips(permanent).map_err(err)
}

#[tauri::command]
fn update_clip_tags(state: tauri::State<AppState>, id: String, tags: String) -> Result<(), String> {
    state.db.lock().unwrap().update_clip_tags(&id, &tags).map_err(err)
}

#[tauri::command]
fn update_clip_folder(state: tauri::State<AppState>, id: String, folder: String) -> Result<(), String> {
    state.db.lock().unwrap().update_clip_folder(&id, &folder).map_err(err)
}

#[tauri::command]
fn update_clip_favorite(state: tauri::State<AppState>, id: String, favorite: bool) -> Result<(), String> {
    state.db.lock().unwrap().update_clip_favorite(&id, favorite).map_err(err)
}

#[tauri::command]
fn delete_clip(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    if let Ok(clip) = db.get_clip(&id) {
        std::fs::remove_file(&clip.path).ok();
        if let Some(thumb) = &clip.thumbnail_path {
            std::fs::remove_file(thumb).ok();
        }
        editor::remove_audio_tracks_sidecar(&clip.path).ok();
    }
    db.delete_clip(&id).map_err(err)
}

#[tauri::command]
fn rename_clip(state: tauri::State<AppState>, id: String, new_name: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let clip = db.get_clip(&id).map_err(err)?;

    let old_path = Path::new(&clip.path);
    let ext = old_path.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let new_filename = if new_name.ends_with(&format!(".{ext}")) {
        new_name.clone()
    } else {
        format!("{new_name}.{ext}")
    };
    let new_path = old_path.parent().unwrap_or(Path::new(".")).join(&new_filename);

    std::fs::rename(old_path, &new_path).map_err(err)?;
    editor::rename_audio_tracks_sidecar(&clip.path, &new_path.to_string_lossy()).map_err(err)?;
    db.rename_clip(&id, &new_filename, &new_path.to_string_lossy()).map_err(err)
}

// Recording -------------------------------------------------------------------

#[tauri::command]
fn start_replay_buffer(state: tauri::State<AppState>) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    state.recorder.lock().unwrap().start_replay_buffer(&s).map_err(err)
}

#[tauri::command]
fn stop_replay_buffer(state: tauri::State<AppState>) -> Result<(), String> {
    state.recorder.lock().unwrap().stop_replay_buffer().map_err(err)
}

#[tauri::command]
fn save_replay(state: tauri::State<AppState>) -> Result<(), String> {
    state.recorder.lock().unwrap().save_replay().map_err(err)
}

#[tauri::command]
fn start_recording(state: tauri::State<AppState>) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    state.recorder.lock().unwrap().start_recording(&s).map_err(err)
}

#[tauri::command]
fn stop_recording(state: tauri::State<AppState>) -> Result<String, String> {
    state.recorder.lock().unwrap().stop_recording().map_err(err)
}

#[tauri::command]
fn get_recording_state(state: tauri::State<AppState>) -> RecordingState {
    state.recorder.lock().unwrap().get_state()
}

// Editor ----------------------------------------------------------------------

#[tauri::command]
async fn trim_clip(
    input: String,
    output: String,
    start: f64,
    end: f64,
    enabled_audio_tracks: Option<Vec<usize>>,
) -> Result<String, String> {
    editor::trim(&input, &output, start, end, enabled_audio_tracks.as_deref()).map_err(err)
}

#[tauri::command]
async fn crop_clip(input: String, output: String, x: u32, y: u32, w: u32, h: u32) -> Result<String, String> {
    editor::crop(&input, &output, x, y, w, h).map_err(err)
}

#[tauri::command]
async fn get_clip_audio_tracks(path: String) -> Vec<editor::ClipAudioTrack> {
    editor::get_audio_tracks(&path)
}

#[tauri::command]
async fn transcode_for_preview(input: String, enabled_audio_tracks: Option<Vec<usize>>) -> Result<String, String> {
    // If it's already mp4 with aac audio, skip entirely — serve the original
    if enabled_audio_tracks.is_none()
        && input.ends_with(".mp4")
        && has_aac_audio(&input)
        && editor::audio_stream_count(&input) <= 1
    {
        return Ok(input);
    }

    let dir = std::env::temp_dir().join(PREVIEW_DIR);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let stem = Path::new(&input).file_stem().unwrap_or_default().to_string_lossy();
    let track_key = enabled_audio_tracks
        .as_ref()
        .map(|tracks| {
            if tracks.is_empty() {
                "muted".into()
            } else {
                tracks.iter().map(usize::to_string).collect::<Vec<_>>().join("-")
            }
        })
        .unwrap_or_else(|| "all".into());
    let out = dir.join(format!("{stem}-{track_key}.mp4"));
    if out.exists() {
        return Ok(out.to_string_lossy().into());
    }

    editor::render_browser_copy(
        &input,
        &out.to_string_lossy(),
        enabled_audio_tracks.as_deref(),
    )
    .map_err(err)
}

fn has_aac_audio(path: &str) -> bool {
    std::process::Command::new("ffprobe")
        .args(["-v", "quiet", "-select_streams", "a:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0", path])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "aac")
        .unwrap_or(false)
}

// R2 -------------------------------------------------------------------------

#[tauri::command]
async fn upload_clip(state: tauri::State<'_, AppState>, id: String, permanent: bool) -> Result<String, String> {
    let s = state.settings.lock().unwrap().clone();
    let clip = state.db.lock().unwrap().get_clip(&id).map_err(err)?;
    if let Some(url) = clip.r2_url.as_ref() {
        return Ok(url.clone());
    }
    let url = r2::upload(&s, &clip, permanent).await.map_err(err)?;
    let expiry = (!permanent).then(|| Utc::now() + Duration::days(s.expiry_days as i64));
    state.db.lock().unwrap().mark_uploaded(&id, &url, permanent, expiry).map_err(err)?;
    Ok(url)
}

#[tauri::command]
async fn delete_from_r2(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    let clip = state.db.lock().unwrap().get_clip(&id).map_err(err)?;
    let key = clip.r2_key.ok_or("clip not uploaded")?;
    r2::delete(&s, &key).await.map_err(err)?;
    state.db.lock().unwrap().mark_deleted(&id).map_err(err)
}

#[tauri::command]
async fn r2_storage(state: tauri::State<'_, AppState>, permanent: bool) -> Result<u64, String> {
    let s = state.settings.lock().unwrap().clone();
    if s.r2_bucket.is_empty() { return Ok(0); }
    let prefix = if permanent { "p/" } else { "t/" };
    r2::storage_usage(&s, prefix).await.map_err(err)
}

// Settings -------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_window_opacity(opacity: f64) -> Result<(), String> {
    let val = opacity.clamp(0.1, 1.0);
    let rule = format!("{val:.2} override {val:.2} override");
    std::process::Command::new("hyprctl")
        .args(["eval", &format!("hl.window_rule({{ match = {{ class = '^(klyppd)$' }}, opacity = '{rule}' }})")])
        .output()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
fn save_settings(state: tauri::State<AppState>, new_settings: AppSettings) -> Result<(), String> {
    *state.settings.lock().unwrap() = new_settings.clone();
    settings::save(&new_settings).map_err(err)
}

#[tauri::command]
fn list_audio_input_devices() -> Result<Vec<AudioDeviceOption>, String> {
    let output = std::process::Command::new("gpu-screen-recorder")
        .arg("--list-audio-devices")
        .output()
        .map_err(err)?;

    let mut devices = Vec::new();
    let mut seen = HashSet::new();
    push_audio_device(
        &mut devices,
        &mut seen,
        "default_input",
        "Default input",
    );

    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    for line in text.lines() {
        if let Some(device) = parse_audio_input_line(line) {
            let label = device.label.clone();
            push_audio_device(&mut devices, &mut seen, &device.value, &label);
        }
    }

    Ok(devices)
}

fn parse_audio_input_line(line: &str) -> Option<AudioDeviceOption> {
    let line = line.trim();
    if line.is_empty()
        || line.starts_with("gsr ")
        || line.starts_with("usage:")
        || line.starts_with("Run ")
        || line.starts_with("Audio ")
        || line.starts_with("NOTES:")
        || line.starts_with("expected one of:")
    {
        return None;
    }

    let (value, raw_label, structured) = if let Some((value, label)) = line.split_once('|') {
        (value.trim(), label.trim(), true)
    } else {
        let value = line.split_whitespace().next()?.trim();
        (value, line[value.len()..].trim(), false)
    };
    let label = raw_label
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(raw_label)
        .trim();

    if !is_audio_input_value(value, label, structured) {
        return None;
    }

    Some(AudioDeviceOption {
        value: value.into(),
        label: if label.is_empty() { value.into() } else { label.into() },
    })
}

fn is_audio_input_value(value: &str, label: &str, structured: bool) -> bool {
    let label = label.to_lowercase();
    if value == "default_input" || value.starts_with("alsa_input.") {
        return true;
    }
    if value.is_empty()
        || value == "default_output"
        || value.starts_with("alsa_output.")
        || value.ends_with(".monitor")
        || label.starts_with("monitor of")
    {
        return false;
    }
    if structured {
        return true;
    }

    let value = value.to_lowercase();
    let source_shaped = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    source_shaped && (value.contains("input") || value.contains("mic") || value.contains("source"))
}

fn push_audio_device(
    devices: &mut Vec<AudioDeviceOption>,
    seen: &mut HashSet<String>,
    value: &str,
    label: &str,
) {
    if seen.insert(value.to_string()) {
        devices.push(AudioDeviceOption {
            value: value.into(),
            label: label.into(),
        });
    }
}

#[tauri::command]
fn get_storage_usage(state: tauri::State<AppState>) -> Result<u64, String> {
    let dir = state.settings.lock().unwrap().clips_directory.clone();
    Ok(std::fs::read_dir(&dir).map(|entries| {
        entries.flatten()
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .sum()
    }).unwrap_or(0))
}

#[tauri::command]
fn get_theme_css() -> Result<String, String> {
    let path = config_dir().join("theme.css");
    Ok(std::fs::read_to_string(path).unwrap_or_default())
}

#[tauri::command]
fn save_theme_css(css: String) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::fs::write(dir.join("theme.css"), css).map_err(err)
}

// Files -----------------------------------------------------------------------

#[tauri::command]
fn read_thumbnail(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(err)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

#[tauri::command]
fn read_video_bytes(path: String) -> Result<Vec<u8>, String> {
    std::fs::read(&path).map_err(err)
}

/// Serve a video file on a random localhost port, return the URL.
#[tauri::command]
fn serve_video(path: String) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(err)?;
    let port = listener.local_addr().map_err(err)?.port();
    let url = format!("http://127.0.0.1:{port}/video.mp4");

    std::thread::spawn(move || {
        // Serve exactly one request then exit
        let file_data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => return,
        };
        // Accept up to 5 requests (initial + range requests for seeking)
        for _ in 0..20 {
            let Ok((mut stream, _)) = listener.accept() else { break; };
            let mut req = vec![0u8; 4096];
            let n = stream.read(&mut req).unwrap_or(0);
            let req_str = String::from_utf8_lossy(&req[..n]);

            // Parse Range header
            let (start, end) = if let Some(range_line) = req_str.lines().find(|l| l.starts_with("Range:")) {
                // Range: bytes=START-END
                let range = range_line.trim_start_matches("Range: bytes=");
                let parts: Vec<&str> = range.split('-').collect();
                let s: usize = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
                let e: usize = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(file_data.len() - 1);
                (s, e.min(file_data.len() - 1))
            } else {
                (0, file_data.len() - 1)
            };

            let chunk = &file_data[start..=end];
            let total = file_data.len();

            let header = if start == 0 && end == total - 1 {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {total}\r\nAccess-Control-Allow-Origin: *\r\nAccept-Ranges: bytes\r\n\r\n"
                )
            } else {
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccept-Ranges: bytes\r\n\r\n",
                    chunk.len()
                )
            };

            stream.write_all(header.as_bytes()).ok();
            stream.write_all(chunk).ok();
        }
    });

    Ok(url)
}

#[tauri::command]
fn replace_file(src: String, dst: String) -> Result<(), String> {
    std::fs::rename(&src, &dst).or_else(|_| {
        std::fs::copy(&src, &dst).map_err(err)?;
        std::fs::remove_file(&src).map_err(err)
    })
}

#[tauri::command]
fn scan_clips(state: tauri::State<AppState>) -> Result<Vec<Clip>, String> {
    let settings = state.settings.lock().unwrap();
    let db = state.db.lock().unwrap();

    let existing: std::collections::HashSet<String> = db.get_all_clips()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.path)
        .collect();

    let thumb_dir = Path::new(&settings.clips_directory).join(".thumbs");
    std::fs::create_dir_all(&thumb_dir).ok();

    if let Ok(entries) = std::fs::read_dir(&settings.clips_directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "mkv" | "mp4" | "webm") { continue; }

            let path_str = path.to_string_lossy().to_string();
            if existing.contains(&path_str) { continue; }

            let created = entry.metadata().ok()
                .and_then(|m| m.created().ok())
                .map(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339())
                .unwrap_or_else(|| Utc::now().to_rfc3339());

            let thumb = thumb_dir.join(format!("{}.jpg", uuid::Uuid::new_v4()));
            let thumb_path = editor::generate_thumbnail(&path_str, &thumb.to_string_lossy())
                .ok()
                .map(|_| thumb.to_string_lossy().to_string());

            let clip = Clip {
                id: uuid::Uuid::new_v4().to_string(),
                filename: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                window_name: None,
                path: path_str,
                duration: editor::get_duration(&path.to_string_lossy()).unwrap_or(0.0),
                created_at: created,
                thumbnail_path: thumb_path,
                tags: None,
                folder: None,
                favorite: false,
                upload_status: "local".into(),
                r2_key: None,
                r2_url: None,
                expiry_date: None,
                is_permanent: false,
            };
            db.insert_clip(&clip).ok();
        }
    }

    db.get_all_clips().map_err(err)
}

// Helpers --------------------------------------------------------------------

fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("klyppd")
}

fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("klyppd")
        .join("klyppd.sock")
}

fn capture_window_class() -> String {
    let out = std::process::Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok();

    let class = out
        .as_ref()
        .and_then(|o| extract_json_string(&String::from_utf8_lossy(&o.stdout), "class"))
        .filter(|s| !s.is_empty());

    match class {
        Some(c) => pretty_app_name(&c),
        None => "Clip".into(),
    }
}

fn extract_json_string(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let start = s.find(&needle)? + needle.len();
    let rest = &s[start..];
    let open = rest.find('"')? + 1;
    let close = rest[open..].find('"')?;
    Some(rest[open..open + close].into())
}

/// Resolve a WM class to a human name via .desktop lookup.
/// `org.vinegarhq.Sober` → `Sober`, `com.spotify.Client` → `Spotify`.
fn pretty_app_name(class: &str) -> String {
    if let Some(name) = lookup_desktop_name(class) {
        return sanitize(&name);
    }

    let segments: Vec<&str> = class.rsplit('.').collect();
    for seg in &segments {
        let lower = seg.to_lowercase();
        if !matches!(lower.as_str(), "client" | "app" | "desktop" | "main") {
            return sanitize(seg);
        }
    }
    sanitize(class)
}

fn sanitize(s: &str) -> String {
    let cleaned: String = s.chars()
        .filter(|c| c.is_alphanumeric() || matches!(*c, '_' | '-'))
        .take(32)
        .collect();
    if cleaned.is_empty() { return "Clip".into(); }
    let mut iter = cleaned.chars();
    iter.next().unwrap().to_uppercase().chain(iter).collect()
}

fn lookup_desktop_name(class: &str) -> Option<String> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Some(d) = dirs::data_dir() {
        dirs.push(d.join("applications"));
    }

    let target = class.to_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue; };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") { continue; }

            let Ok(text) = std::fs::read_to_string(&path) else { continue; };
            let stem_match = path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase() == target)
                .unwrap_or(false);

            let wm_match = text.lines()
                .filter_map(|l| l.strip_prefix("StartupWMClass="))
                .any(|v| v.trim().to_lowercase() == target);

            if !stem_match && !wm_match { continue; }

            if let Some(name) = text.lines().filter_map(|l| l.strip_prefix("Name=")).next() {
                return Some(name.trim().into());
            }
        }
    }
    None
}

fn notify_desktop(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "klyppd", "-i", "video-x-generic", "-t", "2000", title, body])
        .status();
}

fn toast<R: tauri::Runtime>(handle: &tauri::AppHandle<R>, msg: &str, kind: &str) {
    let _ = handle.emit("toast", json!({ "msg": msg, "kind": kind }));
}

// Hotkey dispatcher (called via Unix socket from Hyprland binds) -------------

fn handle_hotkey(handle: &tauri::AppHandle, cmd: &str) {
    let state = handle.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();

    match cmd {
        "save-replay" => {
            let win = capture_window_class();
            let date = Local::now().format("%Y-%m-%d").to_string();
            std::fs::write(PENDING_NAME_PATH, format!("{win}_{date}")).ok();

            let mut recorder = state.recorder.lock().unwrap();
            let tracks = recorder.replay_tracks();
            if !tracks.is_empty() {
                serde_json::to_vec_pretty(&tracks)
                    .ok()
                    .and_then(|bytes| std::fs::write(PENDING_AUDIO_TRACKS_PATH, bytes).ok());
            }

            match recorder.save_replay() {
                Ok(_) => {
                    let body = format!("Klyppd the last {}s of {}", settings.buffer_seconds, win);
                    toast(handle, &body, "ok");
                    notify_desktop("Klypp saved", &body);
                }
                Err(_) => {
                    std::fs::remove_file(PENDING_AUDIO_TRACKS_PATH).ok();
                    toast(handle, "Buffer not running", "err");
                    notify_desktop("Klyppd", "Buffer not running");
                }
            }
        }
        "toggle-buffer" => {
            let mut rec = state.recorder.lock().unwrap();
            if rec.get_state().replay_buffer_active {
                let _ = rec.stop_replay_buffer();
                drop(rec);
                toast(handle, "Klyppd stopped", "ok");
                notify_desktop("Klyppd stopped", "Buffer is no longer recording");
            } else {
                let res = rec.start_replay_buffer(&settings);
                drop(rec);
                match res {
                    Ok(_) => {
                        let body = format!("Buffering last {}s", settings.buffer_seconds);
                        toast(handle, &body, "ok");
                        notify_desktop("Klyppd started", &body);
                    }
                    Err(_) => toast(handle, "Klyppd failed to start", "err"),
                }
            }
        }
        "toggle-recording" => {
            let mut rec = state.recorder.lock().unwrap();
            if rec.get_state().recording_active {
                let _ = rec.stop_recording();
                drop(rec);
                toast(handle, "Klypp recording stopped", "ok");
                notify_desktop("Klypp saved", "Recording stopped");
            } else {
                let res = rec.start_recording(&settings);
                drop(rec);
                match res {
                    Ok(_) => {
                        toast(handle, "Klypping…", "ok");
                        notify_desktop("Klypping…", "Press again to stop");
                    }
                    Err(_) => toast(handle, "Klypp failed", "err"),
                }
            }
        }
        _ => {}
    }
}

// Background workers ---------------------------------------------------------

fn spawn_tray(handle: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show_item = MenuItem::with_id(&handle, "show", "Open klyppd", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(&handle, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(&handle, &[&show_item, &quit_item])?;

    let _ = TrayIconBuilder::with_id("klyppd-tray")
        .icon(handle.default_window_icon().cloned().unwrap())
        .tooltip("klyppd")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(&handle)?;

    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Reads /dev/input/ via evdev for global hotkeys — works on any compositor.
/// User needs to be in the `input` group: `sudo usermod -aG input $USER`
fn spawn_evdev_hotkeys(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        use evdev::{Device, InputEventKind, Key};
        use std::time::Instant;

        // Find keyboard devices
        let mut devices: Vec<Device> = evdev::enumerate()
            .filter_map(|(_, d)| {
                if d.supported_keys().is_some_and(|k| k.contains(Key::KEY_A)) {
                    Some(d)
                } else {
                    None
                }
            })
            .collect();

        if devices.is_empty() {
            eprintln!("klyppd: no keyboard found in /dev/input/ (are you in the 'input' group?)");
            // Notify the user visually — they probably launched from rofi and can't see stderr
            notify_desktop("Klyppd", "Hotkeys unavailable — add yourself to the 'input' group and relog");
            let _ = handle.emit("toast", serde_json::json!({
                "msg": "Hotkeys unavailable — run: sudo usermod -aG input $USER then relog",
                "kind": "err"
            }));
            return;
        }

        // Poll all keyboards
        let mut last_clip = Instant::now() - std::time::Duration::from_secs(5);
        loop {
            for dev in &mut devices {
                if let Ok(events) = dev.fetch_events() {
                    for ev in events {
                        if let InputEventKind::Key(key) = ev.kind() {
                            // Track modifier state
                            update_modifiers(key, ev.value() != 0);

                            if ev.value() != 1 { continue; } // key down only

                            let state = handle.state::<AppState>();
                            let settings = state.settings.lock().unwrap().clone();

                            let matched = if hotkey_matches(&settings.hotkey_save_replay, key) {
                                if last_clip.elapsed() > std::time::Duration::from_millis(1500) {
                                    last_clip = Instant::now();
                                    Some("save-replay")
                                } else { None }
                            } else if hotkey_matches(&settings.hotkey_start_stop_recording, key) {
                                Some("toggle-recording")
                            } else if hotkey_matches(&settings.hotkey_start_stop_buffer, key) {
                                Some("toggle-buffer")
                            } else {
                                None
                            };

                            if let Some(cmd) = matched {
                                handle_hotkey(&handle, cmd);
                            }
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
}

fn key_matches(hotkey_str: &str, pressed: evdev::Key) -> bool {
    // Parse "Alt+R" or "Ctrl+Shift+F9" or just "F9"
    let parts: Vec<&str> = hotkey_str.split('+').map(|s| s.trim()).collect();
    let main_key = parts.last().unwrap_or(&"");
    let key_name = format!("KEY_{}", main_key.to_uppercase());
    format!("{:?}", pressed) == key_name
}

/// Track modifier state globally for combo hotkeys
static MODS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
const MOD_ALT: u8 = 1;
const MOD_CTRL: u8 = 2;
const MOD_SHIFT: u8 = 4;
const MOD_SUPER: u8 = 8;

fn update_modifiers(key: evdev::Key, down: bool) {
    use std::sync::atomic::Ordering;
    let bit = match key {
        evdev::Key::KEY_LEFTALT | evdev::Key::KEY_RIGHTALT => MOD_ALT,
        evdev::Key::KEY_LEFTCTRL | evdev::Key::KEY_RIGHTCTRL => MOD_CTRL,
        evdev::Key::KEY_LEFTSHIFT | evdev::Key::KEY_RIGHTSHIFT => MOD_SHIFT,
        evdev::Key::KEY_LEFTMETA | evdev::Key::KEY_RIGHTMETA => MOD_SUPER,
        _ => return,
    };
    if down {
        MODS.fetch_or(bit, Ordering::Relaxed);
    } else {
        MODS.fetch_and(!bit, Ordering::Relaxed);
    }
}

fn hotkey_matches(hotkey_str: &str, pressed: evdev::Key) -> bool {
    use std::sync::atomic::Ordering;

    let parts: Vec<&str> = hotkey_str.split('+').map(|s| s.trim()).collect();
    if parts.is_empty() { return false; }

    let main_key = parts.last().unwrap().to_uppercase();
    let key_name = format!("KEY_{}", main_key);
    if format!("{:?}", pressed) != key_name { return false; }

    let mods = MODS.load(Ordering::Relaxed);
    let need_alt = parts.iter().any(|p| p.eq_ignore_ascii_case("alt"));
    let need_ctrl = parts.iter().any(|p| p.eq_ignore_ascii_case("ctrl"));
    let need_shift = parts.iter().any(|p| p.eq_ignore_ascii_case("shift"));
    let need_super = parts.iter().any(|p| p.eq_ignore_ascii_case("super") || p.eq_ignore_ascii_case("meta"));

    (need_alt == (mods & MOD_ALT != 0))
        && (need_ctrl == (mods & MOD_CTRL != 0))
        && (need_shift == (mods & MOD_SHIFT != 0))
        && (need_super == (mods & MOD_SUPER != 0))
}

fn spawn_socket_listener(handle: tauri::AppHandle) {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    std::thread::spawn(move || {
        let path = socket_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else { return; };

        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = String::new();
            if s.read_to_string(&mut buf).is_err() { continue; }
            handle_hotkey(&handle, buf.trim());
        }
    });
}

fn spawn_clips_watcher(handle: tauri::AppHandle, dir: String) {
    use std::collections::HashMap;
    use std::time::{Duration as StdDuration, Instant};
    use notify::{EventKind, RecursiveMode, Watcher};

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let Ok(mut watcher) = notify::recommended_watcher(tx) else { return; };
        if watcher.watch(Path::new(&dir), RecursiveMode::NonRecursive).is_err() { return; }

        let mut seen: HashMap<String, Instant> = HashMap::new();

        for event in rx.iter().flatten() {
            if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) { continue; }

            for path in event.paths {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "mkv" | "mp4" | "webm") { continue; }

                let key = path.to_string_lossy().to_string();
                if seen.get(&key).is_some_and(|t| t.elapsed() < StdDuration::from_secs(60)) {
                    continue;
                }
                seen.insert(key, Instant::now());
                seen.retain(|_, t| t.elapsed() < StdDuration::from_secs(300));

                std::thread::sleep(StdDuration::from_millis(1200));

                let final_path = rename_if_pending(&path, ext).unwrap_or(path);
                write_pending_audio_tracks(&final_path);
                let filename = final_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let _ = handle.emit("clip-saved", json!({
                    "filename": filename,
                    "path": final_path.to_string_lossy(),
                }));
            }
        }
    });
}

fn rename_if_pending(path: &Path, ext: &str) -> Option<PathBuf> {
    let pending = std::fs::read_to_string(PENDING_NAME_PATH).ok()?;
    let pending = pending.trim();
    if pending.is_empty() { return None; }

    let parent = path.parent().unwrap_or(Path::new("."));
    let mut target = parent.join(format!("{pending}.{ext}"));
    let mut n = 1;
    while target.exists() {
        target = parent.join(format!("{pending}_{n}.{ext}"));
        n += 1;
    }

    let result = std::fs::rename(path, &target).ok().map(|_| target);
    std::fs::remove_file(PENDING_NAME_PATH).ok();
    result
}

fn write_pending_audio_tracks(path: &Path) {
    let Ok(bytes) = std::fs::read(PENDING_AUDIO_TRACKS_PATH) else { return; };
    let Ok(tracks) = serde_json::from_slice::<Vec<editor::AudioTrackMeta>>(&bytes) else {
        std::fs::remove_file(PENDING_AUDIO_TRACKS_PATH).ok();
        return;
    };

    editor::write_audio_tracks_sidecar(&path.to_string_lossy(), &tracks).ok();
    std::fs::remove_file(PENDING_AUDIO_TRACKS_PATH).ok();
}

pub fn purge_non_temp_r2() -> Result<(usize, usize), String> {
    let settings = settings::load().map_err(err)?;
    let data_dir = dirs::data_dir().unwrap_or_default().join("klyppd");
    std::fs::create_dir_all(&data_dir).map_err(err)?;
    let db = Database::new(&data_dir.join("clips.db")).map_err(err)?;
    let clips = db.get_uploaded_clips(true).map_err(err)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(err)?;

    let mut deleted = 0usize;
    let mut failed = 0usize;
    for clip in clips {
        let Some(key) = clip.r2_key.as_deref() else {
            failed += 1;
            continue;
        };
        let removed = rt.block_on(r2::delete(&settings, key)).is_ok();
        if removed && db.mark_deleted(&clip.id).is_ok() {
            deleted += 1;
        } else {
            failed += 1;
        }
    }

    let (extra_deleted, extra_failed) = rt
        .block_on(r2::purge_non_temp_objects(&settings))
        .map_err(err)?;
    deleted += extra_deleted;
    failed += extra_failed;

    Ok((deleted, failed))
}

pub fn purge_orphan_temp_r2() -> Result<(usize, usize), String> {
    let settings = settings::load().map_err(err)?;
    let data_dir = dirs::data_dir().unwrap_or_default().join("klyppd");
    std::fs::create_dir_all(&data_dir).map_err(err)?;
    let db = Database::new(&data_dir.join("clips.db")).map_err(err)?;
    let clips = db.get_uploaded_clips(false).map_err(err)?;

    let keep: HashSet<String> = clips
        .into_iter()
        .filter_map(|clip| clip.r2_key)
        .map(|k| {
            let k = k.strip_suffix(".html").unwrap_or(&k);
            let k = k.strip_suffix(".mp4").unwrap_or(k);
            let k = k.strip_suffix(".mkv").unwrap_or(k);
            let k = k.strip_suffix(".webm").unwrap_or(k);
            let k = k.strip_suffix(".jpg").unwrap_or(k);
            k.to_string()
        })
        .collect();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(err)?;

    rt.block_on(r2::purge_orphan_temp_objects(&settings, &keep))
        .map_err(err)
}

// Entry point ----------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::fs::create_dir_all(config_dir()).ok();

    let data_dir = dirs::data_dir().unwrap_or_default().join("klyppd");
    std::fs::create_dir_all(&data_dir).ok();

    let settings = settings::load().unwrap_or_default();
    std::fs::create_dir_all(&settings.clips_directory).ok();
    let watch_dir = settings.clips_directory.clone();

    let db = Database::new(&data_dir.join("clips.db")).expect("open clips db");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState {
            db: Mutex::new(db),
            recorder: Mutex::new(Recorder::new()),
            settings: Mutex::new(settings),
        })
        .setup(move |app| {
            spawn_socket_listener(app.handle().clone());
            spawn_clips_watcher(app.handle().clone(), watch_dir);
            spawn_tray(app.handle().clone())?;
            spawn_evdev_hotkeys(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_clips, get_clips_by_folder, get_uploaded_clips,
            update_clip_tags, update_clip_folder, update_clip_favorite, delete_clip, rename_clip,
            start_replay_buffer, stop_replay_buffer, save_replay,
            start_recording, stop_recording, get_recording_state,
            trim_clip, crop_clip, transcode_for_preview,
            get_clip_audio_tracks,
            upload_clip, delete_from_r2, r2_storage,
            get_settings, save_settings, set_window_opacity, get_storage_usage, get_theme_css,
            save_theme_css, list_audio_input_devices,
            scan_clips, read_thumbnail, read_video_bytes, serve_video, replace_file,
        ])
        .run(tauri::generate_context!())
        .expect("run tauri app");
}
