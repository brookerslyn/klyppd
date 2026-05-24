import {
  app,
  BrowserWindow,
  ipcMain,
  Tray,
  Menu,
  clipboard,
  nativeImage,
} from "electron";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import http from "node:http";
import { execSync, execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import * as nodeNet from "node:net";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
import { watch } from "chokidar";
import { v4 as uuidv4 } from "uuid";

import { ClipDatabase } from "./db.js";
import type { Clip } from "./db.js";
import { Recorder } from "./recorder.js";
import {
  loadSettings,
  saveSettings as saveSettingsFile,
  configDir,
  dataDir,
  type AppSettings,
} from "./settings.js";
import * as r2 from "./r2.js";
import * as editor from "./editor.js";
import { startEvdevHotkeys } from "./hotkeys.js";

const PREVIEW_DIR = "klyppd-preview";

function runtimeDir(): string {
  return process.env.XDG_RUNTIME_DIR || "/tmp";
}

function socketPath(): string {
  return path.join(runtimeDir(), "klyppd.sock");
}

function pendingNamePath(): string {
  return path.join(runtimeDir(), "klyppd-pending-name");
}

// ─── State ──────────────────────────────────────────────────────────────

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let db: ClipDatabase;
let recorder: Recorder;
let settings: AppSettings;

// ─── Window helpers ─────────────────────────────────────────────────────

function showMainWindow(): void {
  if (!mainWindow) return;
  mainWindow.show();
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.focus();
}

function whichCmd(name: string): boolean {
  try {
    execSync(`which ${name}`, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

// ─── Compositor helpers ─────────────────────────────────────────────────

function captureWindowClass(): string {
  const c =
    tryHyprlandClass() ??
    trySwayClass() ??
    tryGnomeClass() ??
    tryKdeClass() ??
    tryXdotoolClass();
  return c ? prettyAppName(c) : "Clip";
}

function tryHyprlandClass(): string | null {
  try {
    const out = execSync("hyprctl activewindow -j", { encoding: "utf-8", timeout: 2000 });
    const data = JSON.parse(out);
    return data.class || null;
  } catch {
    return null;
  }
}

function trySwayClass(): string | null {
  try {
    const out = execSync("swaymsg -t get_tree", { encoding: "utf-8", timeout: 2000 });
    const tree = JSON.parse(out);
    return findFocusedAppId(tree);
  } catch {
    return null;
  }
}

function findFocusedAppId(node: Record<string, unknown>): string | null {
  if (node.focused && node.app_id) return node.app_id as string;
  for (const child of (node.nodes as Record<string, unknown>[]) ?? []) {
    const r = findFocusedAppId(child);
    if (r) return r;
  }
  for (const child of (node.floating_nodes as Record<string, unknown>[]) ?? []) {
    const r = findFocusedAppId(child);
    if (r) return r;
  }
  return null;
}

function tryGnomeClass(): string | null {
  try {
    const out = execSync(
      `gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell --method org.gnome.Shell.Eval "global.display.focus_window?.get_wm_class()"`,
      { encoding: "utf-8", timeout: 2000 }
    );
    const match = out.match(/'([^']+)'/);
    return match?.[1] || null;
  } catch {
    return null;
  }
}

function tryKdeClass(): string | null {
  try {
    const out = execSync(
      "qdbus org.kde.KWin /KWin org.kde.KWin.activeClient",
      { encoding: "utf-8", timeout: 2000 }
    );
    return out.trim() || null;
  } catch {
    return null;
  }
}

function tryXdotoolClass(): string | null {
  try {
    const id = execSync("xdotool getactivewindow", { encoding: "utf-8", timeout: 2000 }).trim();
    const out = execSync(`xprop -id ${id} WM_CLASS`, { encoding: "utf-8", timeout: 2000 });
    const match = out.match(/"([^"]+)"\s*$/);
    return match?.[1] || null;
  } catch {
    return null;
  }
}

function prettyAppName(raw: string): string {
  const name = raw.split(".").pop() || raw;
  return name.charAt(0).toUpperCase() + name.slice(1);
}

function lookupDesktopIcon(className: string): string | null {
  const xdgDirs = (process.env.XDG_DATA_DIRS || "/usr/share:/usr/local/share")
    .split(":")
    .concat([path.join(os.homedir(), ".local", "share")]);

  const lower = className.toLowerCase();
  for (const dir of xdgDirs) {
    const appDir = path.join(dir, "applications");
    try {
      const files = fs.readdirSync(appDir);
      const match = files.find(
        (f) => f.toLowerCase().includes(lower) && f.endsWith(".desktop")
      );
      if (match) {
        const contents = fs.readFileSync(path.join(appDir, match), "utf-8");
        const iconLine = contents
          .split("\n")
          .find((l) => l.startsWith("Icon="));
        if (iconLine) return iconLine.slice(5);
      }
    } catch {}
  }
  return null;
}

// ─── Hotkey dispatcher ──────────────────────────────────────────────────

function handleHotkey(cmd: string): void {
  switch (cmd) {
    case "save-replay": {
      const win = captureWindowClass();
      const date = new Date().toISOString().slice(0, 10);
      try {
        fs.writeFileSync(pendingNamePath(), `${win}_${date}`);
      } catch {}

      try {
        recorder.saveReplay();
        const body = `Klyppd the last ${settings.buffer_seconds}s of ${win}`;
        sendToast(body, "ok");
        notifyDesktop("Klypp saved", body);
      } catch {
        sendToast("Buffer not running", "err");
        notifyDesktop("Klyppd", "Buffer not running");
      }
      break;
    }
    case "toggle-buffer": {
      if (recorder.getState().replay_buffer_active) {
        try {
          recorder.stopReplayBuffer();
        } catch {}
        sendToast("Klyppd stopped", "ok");
        notifyDesktop("Klyppd stopped", "Buffer is no longer recording");
      } else {
        try {
          recorder.startReplayBuffer(settings);
          const body = `Buffering last ${settings.buffer_seconds}s`;
          sendToast(body, "ok");
          notifyDesktop("Klyppd started", body);
        } catch {
          sendToast("Klyppd failed to start", "err");
        }
      }
      break;
    }
    case "toggle-recording": {
      if (recorder.getState().recording_active) {
        try {
          recorder.stopRecording();
        } catch {}
        sendToast("Klypp recording stopped", "ok");
        notifyDesktop("Klypp saved", "Recording stopped");
      } else {
        try {
          recorder.startRecording(settings);
          sendToast("Klypping\u2026", "ok");
          notifyDesktop("Klypping\u2026", "Press again to stop");
        } catch {
          sendToast("Klypp failed", "err");
        }
      }
      break;
    }
  }
}

function sendToast(msg: string, kind: string): void {
  mainWindow?.webContents.send("toast", { msg, kind });
}

function notifyDesktop(title: string, body: string): void {
  try {
    execSync(
      `notify-send -a klyppd -i video-x-generic -t 2000 "${title}" "${body}"`,
      { stdio: "ignore" }
    );
  } catch {}
}

// ─── Video server ───────────────────────────────────────────────────────

function serveVideo(filePath: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const stat = fs.statSync(filePath);
    const fileSize = stat.size;

    const server = http.createServer((req, res) => {
      const range = req.headers.range;
      if (range) {
        const parts = range.replace(/bytes=/, "").split("-");
        const start = parseInt(parts[0], 10);
        const end = parts[1] ? parseInt(parts[1], 10) : fileSize - 1;
        const chunkSize = end - start + 1;

        res.writeHead(206, {
          "Content-Range": `bytes ${start}-${end}/${fileSize}`,
          "Accept-Ranges": "bytes",
          "Content-Length": chunkSize,
          "Content-Type": "video/mp4",
          "Access-Control-Allow-Origin": "*",
        });
        fs.createReadStream(filePath, { start, end }).pipe(res);
      } else {
        res.writeHead(200, {
          "Content-Length": fileSize,
          "Content-Type": "video/mp4",
          "Accept-Ranges": "bytes",
          "Access-Control-Allow-Origin": "*",
        });
        fs.createReadStream(filePath).pipe(res);
      }
    });

    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (addr && typeof addr !== "string") {
        resolve(`http://127.0.0.1:${addr.port}/video.mp4`);
      } else {
        reject(new Error("Failed to start video server"));
      }
    });

    // Auto-close after 5 minutes of inactivity
    let timeout = setTimeout(() => server.close(), 5 * 60 * 1000);
    server.on("request", () => {
      clearTimeout(timeout);
      timeout = setTimeout(() => server.close(), 5 * 60 * 1000);
    });
  });
}

// ─── Clips watcher ──────────────────────────────────────────────────────

function spawnClipsWatcher(dir: string): void {
  const seen = new Map<string, number>();

  const watcher = watch(dir, {
    ignoreInitial: true,
    depth: 0,
    awaitWriteFinish: { stabilityThreshold: 1200 },
  });

  watcher.on("add", (filePath) => {
    const ext = path.extname(filePath).slice(1);
    if (!["mkv", "mp4", "webm"].includes(ext)) return;

    const now = Date.now();
    const lastSeen = seen.get(filePath) ?? 0;
    if (now - lastSeen < 60000) return;
    seen.set(filePath, now);

    // Clean old entries
    for (const [k, v] of seen) {
      if (now - v > 300000) seen.delete(k);
    }

    const finalPath = renameIfPending(filePath, ext) ?? filePath;
    const filename = path.basename(finalPath);
    mainWindow?.webContents.send("clip-saved", { filename, path: finalPath });
  });
}

function renameIfPending(filePath: string, ext: string): string | null {
  const pnp = pendingNamePath();
  let pending: string;
  try {
    pending = fs.readFileSync(pnp, "utf-8").trim();
  } catch {
    return null;
  }
  if (!pending) return null;

  const dir = path.dirname(filePath);
  let target = path.join(dir, `${pending}.${ext}`);
  let n = 1;
  while (fs.existsSync(target)) {
    target = path.join(dir, `${pending}_${n}.${ext}`);
    n++;
  }

  try {
    fs.renameSync(filePath, target);
    fs.unlinkSync(pnp);
    return target;
  } catch {
    return null;
  }
}

// ─── Socket listener (for legacy compositor binds) ──────────────────────

function spawnSocketListener(): void {
  const sockPath = socketPath();
  try {
    fs.unlinkSync(sockPath);
  } catch {}

  const server = nodeNet.createServer((conn: nodeNet.Socket) => {
    let data = "";
    conn.on("data", (chunk: Buffer) => {
      data += chunk.toString();
    });
    conn.on("end", () => {
      handleHotkey(data.trim());
    });
  });

  server.listen(sockPath);
}

// ─── Preview cache cleanup ──────────────────────────────────────────────

function cleanupPreviewCache(): void {
  const dir = path.join(os.tmpdir(), PREVIEW_DIR);
  if (!fs.existsSync(dir)) return;
  const cutoff = Date.now() - 7 * 24 * 3600 * 1000;

  try {
    for (const entry of fs.readdirSync(dir)) {
      const filePath = path.join(dir, entry);
      try {
        const stat = fs.statSync(filePath);
        if (stat.mtimeMs < cutoff) fs.unlinkSync(filePath);
      } catch {}
    }
  } catch {}
}

// ─── IPC Handlers ───────────────────────────────────────────────────────

function registerIpcHandlers(): void {
  // Library queries
  ipcMain.handle("get_clips", () => db.getAllClips());
  ipcMain.handle("get_clips_by_folder", (_, folder: string) =>
    db.getClipsByFolder(folder)
  );
  ipcMain.handle("get_uploaded_clips", (_, permanent: boolean) =>
    db.getUploadedClips(permanent)
  );
  ipcMain.handle("update_clip_tags", (_, id: string, tags: string) =>
    db.updateClipTags(id, tags)
  );
  ipcMain.handle("update_clip_folder", (_, id: string, folder: string) =>
    db.updateClipFolder(id, folder)
  );
  ipcMain.handle("delete_clip", (_, id: string) => db.deleteClip(id));
  ipcMain.handle("rename_clip", (_, id: string, newName: string) => {
    const clip = db.getClip(id);
    const oldPath = clip.path;
    const ext = path.extname(oldPath).slice(1) || "mp4";
    const newFilename = newName.endsWith(`.${ext}`)
      ? newName
      : `${newName}.${ext}`;
    const newPath = path.join(path.dirname(oldPath), newFilename);
    fs.renameSync(oldPath, newPath);
    db.renameClip(id, newFilename, newPath);
  });

  // Recording
  ipcMain.handle("start_replay_buffer", () =>
    recorder.startReplayBuffer(settings)
  );
  ipcMain.handle("stop_replay_buffer", () => recorder.stopReplayBuffer());
  ipcMain.handle("save_replay", () => recorder.saveReplay());
  ipcMain.handle("start_recording", () => recorder.startRecording(settings));
  ipcMain.handle("stop_recording", () => recorder.stopRecording());
  ipcMain.handle("get_recording_state", () => recorder.getState());

  // Editor
  ipcMain.handle(
    "trim_clip",
    (_, input: string, output: string, start: number, end: number) =>
      editor.trim(input, output, start, end)
  );
  ipcMain.handle(
    "crop_clip",
    (
      _,
      input: string,
      output: string,
      x: number,
      y: number,
      w: number,
      h: number
    ) => editor.crop(input, output, x, y, w, h)
  );
  ipcMain.handle("transcode_for_preview", async (_, input: string) => {
    if (input.endsWith(".mp4") && editor.hasAacAudio(input)) return input;

    const dir = path.join(os.tmpdir(), PREVIEW_DIR);
    fs.mkdirSync(dir, { recursive: true });
    const stem = path.basename(input, path.extname(input));
    const out = path.join(dir, `${stem}.mp4`);
    if (fs.existsSync(out)) return out;

    try {
      execFileSync("ffmpeg", [
        "-y",
        "-i",
        input,
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-b:a",
        "160k",
        "-movflags",
        "+faststart",
        out,
      ]);
      return out;
    } catch {
      throw new Error("ffmpeg transcode failed");
    }
  });

  // R2
  ipcMain.handle("upload_clip", async (_, id: string, permanent: boolean) => {
    const clip = db.getClip(id);
    const url = await r2.upload(settings, clip, permanent);
    const expiry = !permanent
      ? new Date(
          Date.now() + settings.expiry_days * 24 * 3600 * 1000
        ).toISOString()
      : null;
    db.markUploaded(id, url, permanent, expiry);
    return url;
  });
  ipcMain.handle("delete_from_r2", async (_, id: string) => {
    const clip = db.getClip(id);
    if (!clip.r2_key) throw new Error("clip not uploaded");
    await r2.deleteFromR2(settings, clip.r2_key);
    db.markDeleted(id);
  });
  ipcMain.handle("r2_storage", async (_, permanent: boolean) => {
    if (!settings.r2_bucket) return 0;
    const prefix = permanent ? "p/" : "t/";
    return r2.storageUsage(settings, prefix);
  });

  // Settings
  ipcMain.handle("get_settings", () => settings);
  ipcMain.handle("save_settings", (_, newSettings: AppSettings) => {
    settings = newSettings;
    saveSettingsFile(newSettings);
  });
  ipcMain.handle("set_window_opacity", (_, opacity: number) => {
    const val = Math.min(1.0, Math.max(0.1, opacity));
    if (whichCmd("hyprctl")) {
      const rule = `${val.toFixed(2)} override ${val.toFixed(2)} override`;
      try {
        execSync(
          `hyprctl eval "hl.window_rule({ match = { class = '^(klyppd)$' }, opacity = '${rule}' })"`,
          { stdio: "ignore" }
        );
      } catch {}
    } else if (whichCmd("swaymsg")) {
      const pct = Math.round(val * 100).toString();
      try {
        execSync(`swaymsg "[app_id=klyppd]" opacity ${pct}`, {
          stdio: "ignore",
        });
      } catch {}
    }
  });
  ipcMain.handle("get_storage_usage", () => {
    const dir = settings.clips_directory;
    try {
      return fs
        .readdirSync(dir)
        .reduce((sum, f) => {
          try {
            return sum + fs.statSync(path.join(dir, f)).size;
          } catch {
            return sum;
          }
        }, 0);
    } catch {
      return 0;
    }
  });
  ipcMain.handle("get_theme_css", () => {
    const themePath = path.join(configDir(), "theme.css");
    try {
      return fs.readFileSync(themePath, "utf-8");
    } catch {
      return "";
    }
  });
  ipcMain.handle("check_dependencies", () => {
    const missing: string[] = [];
    for (const dep of ["gpu-screen-recorder", "ffmpeg", "ffprobe"]) {
      if (!whichCmd(dep)) missing.push(dep);
    }
    return missing;
  });

  // Files
  ipcMain.handle("read_thumbnail", (_, thumbnailPath: string) => {
    const bytes = fs.readFileSync(thumbnailPath);
    return `data:image/jpeg;base64,${bytes.toString("base64")}`;
  });
  ipcMain.handle("read_video_bytes", (_, videoPath: string) =>
    fs.readFileSync(videoPath)
  );
  ipcMain.handle("serve_video", (_, videoPath: string) =>
    serveVideo(videoPath)
  );
  ipcMain.handle("replace_file", (_, src: string, dst: string) => {
    try {
      fs.renameSync(src, dst);
    } catch {
      fs.copyFileSync(src, dst);
      fs.unlinkSync(src);
    }
  });

  // Scan clips
  ipcMain.handle("scan_clips", () => {
    const clipsDir = settings.clips_directory;

    // Remove stale DB entries
    const allClips = db.getAllClips();
    for (const clip of allClips) {
      if (!fs.existsSync(clip.path)) {
        db.deleteClip(clip.id);
      }
    }

    const existing = new Set(db.getAllClips().map((c) => c.path));
    const thumbDir = path.join(clipsDir, ".thumbs");
    fs.mkdirSync(thumbDir, { recursive: true });

    const newClips: Clip[] = [];

    try {
      for (const entry of fs.readdirSync(clipsDir)) {
        const filePath = path.join(clipsDir, entry);
        const ext = path.extname(filePath).slice(1);
        if (!["mkv", "mp4", "webm"].includes(ext)) continue;
        if (existing.has(filePath)) continue;

        let created: string;
        try {
          const stat = fs.statSync(filePath);
          created = (stat.birthtime || stat.mtime).toISOString();
        } catch {
          created = new Date().toISOString();
        }

        const thumbPath = path.join(thumbDir, `${uuidv4()}.jpg`);
        let thumbResult: string | null = null;
        try {
          editor.generateThumbnail(filePath, thumbPath);
          thumbResult = thumbPath;
        } catch {}

        newClips.push({
          id: uuidv4(),
          filename: entry,
          path: filePath,
          duration: editor.getDuration(filePath),
          created_at: created,
          thumbnail_path: thumbResult,
          tags: null,
          folder: null,
          upload_status: "local",
          r2_key: null,
          r2_url: null,
          expiry_date: null,
          is_permanent: false,
        });
      }
    } catch {}

    for (const clip of newClips) {
      db.insertClip(clip);
    }

    return db.getAllClips();
  });

  // Window controls
  ipcMain.handle("clipboard:writeText", (_, text: string) =>
    clipboard.writeText(text)
  );
  ipcMain.handle("window:hide", () => mainWindow?.hide());
  ipcMain.handle("window:close", () => mainWindow?.close());
}

// ─── App lifecycle ──────────────────────────────────────────────────────

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1100,
    height: 700,
    minWidth: 800,
    minHeight: 500,
    frame: false,
    transparent: false,
    backgroundColor: "#101418",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
    },
    icon: path.join(__dirname, "..", "public", "logo.png"),
  });

  if (process.env.NODE_ENV === "development" || process.env.VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(process.env.VITE_DEV_SERVER_URL || "http://localhost:1420");
  } else {
    mainWindow.loadFile(path.join(__dirname, "..", "build", "index.html"));
  }

  mainWindow.on("close", (e) => {
    e.preventDefault();
    mainWindow?.hide();
  });
}

function createTray(): void {
  const iconPath = path.join(__dirname, "..", "public", "logo.png");
  let icon: Electron.NativeImage;
  try {
    icon = nativeImage.createFromPath(iconPath).resize({ width: 24 });
  } catch {
    icon = nativeImage.createEmpty();
  }

  tray = new Tray(icon);
  tray.setToolTip("klyppd");

  const contextMenu = Menu.buildFromTemplate([
    { label: "Open klyppd", click: showMainWindow },
    { type: "separator" },
    {
      label: "Quit",
      click: () => {
        recorder.cleanup();
        app.exit(0);
      },
    },
  ]);

  tray.setContextMenu(contextMenu);
  tray.on("click", showMainWindow);
}

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on("second-instance", () => showMainWindow());

  // Keep Electron's internal data separate from user config
  app.setPath("userData", path.join(dataDir(), "electron"));

  app.whenReady().then(() => {
    // Initialize
    fs.mkdirSync(configDir(), { recursive: true });
    cleanupPreviewCache();

    const dataPath = dataDir();
    fs.mkdirSync(dataPath, { recursive: true });

    settings = loadSettings();
    console.log("klyppd: config loaded from", configDir());
    console.log("klyppd: clips_directory =", settings.clips_directory);
    console.log("klyppd: hotkey_save_replay =", settings.hotkey_save_replay);
    console.log("klyppd: r2_bucket =", settings.r2_bucket);
    fs.mkdirSync(settings.clips_directory, { recursive: true });

    db = new ClipDatabase(path.join(dataPath, "clips.db"));
    recorder = new Recorder();

    registerIpcHandlers();
    createWindow();
    try { createTray(); } catch (e) { console.error("Tray failed:", e); }

    try { spawnSocketListener(); } catch (e) { console.error("Socket listener failed:", e); }
    try { spawnClipsWatcher(settings.clips_directory); } catch (e) { console.error("Watcher failed:", e); }

    try {
      startEvdevHotkeys(
        () => settings,
        (cmd) => handleHotkey(cmd)
      );
    } catch (e) { console.error("Hotkeys failed:", e); }
  });

  app.on("before-quit", () => {
    recorder?.cleanup();
    db?.close();
    mainWindow?.destroy();
  });

  app.on("window-all-closed", () => {
    // Don't quit on window close — stay in tray
  });
}
