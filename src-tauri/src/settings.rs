use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

type Error = Box<dyn std::error::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub clips_directory: String,
    pub buffer_seconds: u32,
    pub fps: u32,
    pub codec: String,
    pub container: String,
    pub audio_codec: String,
    pub audio_source: String,
    pub audio_microphone_source: String,
    pub audio_focused_window: bool,
    pub audio_discord: bool,
    pub audio_spotify: bool,
    pub audio_microphone: bool,
    pub clipping_sound_enabled: bool,
    pub clipping_sound_path: String,
    pub hotkey_save_replay: String,
    pub hotkey_start_stop_recording: String,
    pub hotkey_start_stop_buffer: String,
    pub r2_endpoint: String,
    pub r2_bucket: String,
    pub r2_access_key: String,
    pub r2_secret_key: String,
    pub r2_custom_domain: String,
    pub expiry_days: u32,
    pub theme_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        let clips_dir = dirs::video_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Videos")))
            .unwrap_or_default()
            .join("Klyppd");

        Self {
            clips_directory: clips_dir.to_string_lossy().into(),
            buffer_seconds: 120,
            fps: 60,
            codec: "h264".into(),
            container: "mp4".into(),
            audio_codec: "aac".into(),
            audio_source: "default_output".into(),
            audio_microphone_source: "default_input".into(),
            audio_focused_window: true,
            audio_discord: false,
            audio_spotify: false,
            audio_microphone: false,
            clipping_sound_enabled: true,
            clipping_sound_path: String::new(),
            hotkey_save_replay: "Alt+R".into(),
            hotkey_start_stop_recording: "Alt+Shift+R".into(),
            hotkey_start_stop_buffer: "Alt+F8".into(),
            r2_endpoint: String::new(),
            r2_bucket: String::new(),
            r2_access_key: String::new(),
            r2_secret_key: String::new(),
            r2_custom_domain: String::new(),
            expiry_days: 14,
            theme_path: String::new(),
        }
    }
}

pub fn safe_stem(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("preview");
    stem.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' })
        .collect()
}

pub fn preview_cache_name(path: &str) -> String {
    let mut seed = path.as_bytes().to_vec();
    if let Ok(meta) = std::fs::metadata(path) {
        seed.extend_from_slice(&meta.len().to_le_bytes());
        if let Ok(modified) = meta.modified() {
            if let Ok(delta) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                seed.extend_from_slice(&delta.as_secs().to_le_bytes());
                seed.extend_from_slice(&delta.subsec_nanos().to_le_bytes());
            }
        }
    }
    format!("{}-{:016x}", safe_stem(path), fnv1a64(&seed))
}

pub fn resolve_user_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    PathBuf::from(path)
}

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("klyppd").join("settings.json")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn load() -> Result<AppSettings, Error> {
    let p = path();
    if !p.exists() {
        let s = AppSettings::default();
        save(&s)?;
        return Ok(s);
    }
    // FIXME: if user hand-edits the JSON and adds a typo, this silently falls back to defaults
    // should probably warn or show a toast in the UI
    Ok(serde_json::from_str(&std::fs::read_to_string(p)?)?)
}

pub fn save(settings: &AppSettings) -> Result<(), Error> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}
