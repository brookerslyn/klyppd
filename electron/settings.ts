import fs from "node:fs";
import path from "node:path";
import os from "node:os";

export interface AppSettings {
  clips_directory: string;
  buffer_seconds: number;
  fps: number;
  codec: string;
  container: string;
  audio_codec: string;
  audio_source: string;
  hotkey_save_replay: string;
  hotkey_start_stop_recording: string;
  hotkey_start_stop_buffer: string;
  r2_endpoint: string;
  r2_bucket: string;
  r2_access_key: string;
  r2_secret_key: string;
  r2_custom_domain: string;
  expiry_days: number;
  theme_path: string;
}

function defaultClipsDir(): string {
  const videos = path.join(os.homedir(), "Videos");
  return path.join(fs.existsSync(videos) ? videos : os.homedir(), "Klyppd");
}

export function defaultSettings(): AppSettings {
  return {
    clips_directory: defaultClipsDir(),
    buffer_seconds: 120,
    fps: 60,
    codec: "h264",
    container: "mp4",
    audio_codec: "aac",
    audio_source: "default_output",
    hotkey_save_replay: "Alt+R",
    hotkey_start_stop_recording: "Alt+Shift+R",
    hotkey_start_stop_buffer: "Alt+F8",
    r2_endpoint: "",
    r2_bucket: "",
    r2_access_key: "",
    r2_secret_key: "",
    r2_custom_domain: "",
    expiry_days: 14,
    theme_path: "",
  };
}

export function configDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  const base = xdg || path.join(os.homedir(), ".config");
  return path.join(base, "klyppd");
}

export function dataDir(): string {
  const xdg = process.env.XDG_DATA_HOME;
  const base = xdg || path.join(os.homedir(), ".local", "share");
  return path.join(base, "klyppd");
}

function settingsPath(): string {
  return path.join(configDir(), "settings.json");
}

export function loadSettings(): AppSettings {
  const p = settingsPath();
  if (!fs.existsSync(p)) {
    const s = defaultSettings();
    saveSettings(s);
    return s;
  }
  try {
    const contents = fs.readFileSync(p, "utf-8");
    return { ...defaultSettings(), ...JSON.parse(contents) };
  } catch (e) {
    console.error(`klyppd: failed to parse ${p}: ${e}`);
    console.error(
      "klyppd: using default settings; fix the JSON or delete the file to regenerate"
    );
    const backup = p.replace(/\.json$/, ".json.bak");
    try {
      fs.copyFileSync(p, backup);
    } catch {}
    const s = defaultSettings();
    saveSettings(s);
    return s;
  }
}

export function saveSettings(settings: AppSettings): void {
  const p = settingsPath();
  const dir = path.dirname(p);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(p, JSON.stringify(settings, null, 2));
}
