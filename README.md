<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
  <source media="(prefers-color-scheme: light)" srcset="assets/logo-light.png">
  <img alt="Klyppd" src="assets/logo-light.png" width="320">
</picture>

**A lightweight, performance-focused clipping and recording app built for speed, customization, and seamless sharing.**

[![Status](https://img.shields.io/badge/status-active%20development-orange)](#project-status)
[![Platform](https://img.shields.io/badge/platform-Linux-blue)](#requirements)
[![Backend](https://img.shields.io/badge/powered%20by-gpu--screen--recorder-green)](https://git.dec05eba.com/gpu-screen-recorder)
[![License](https://img.shields.io/badge/license-MIT-blueviolet)](LICENSE)

</div>

---

## Preview

![Klyppd UI Preview](https://cdn.brookerslyn.space/Klyppd.png)

---

## Overview

Klyppd is a clipping and screen recording tool that prioritizes **low overhead**, **responsiveness**, and **user control**, all while remaining simple to use.

Unlike many existing clipping tools, Klyppd is designed to stay out of the way: minimal performance impact during gameplay, fast access to your most recent moments, and a workflow that gets clips from capture to shareable link in seconds.

---

## Features

### Fast GPU-Accelerated Recording

Klyppd uses [gpu-screen-recorder](https://git.dec05eba.com/gpu-screen-recorder) as its recording backend, enabling efficient clipping with minimal performance impact.

- Save the **last X seconds** of gameplay or desktop activity
- Fullscreen or window-specific recording
- Instant replay-style clipping
- Standard screen recording support
- Smart filenames generated from the active app (`Sober_2026-05-22.mp4`)

### Built-In Video Trimmer

A built-in trimmer powered by [FFmpeg](https://github.com/FFmpeg/FFmpeg) lets you quickly cut clips without opening an external editor.

- Draggable in/out handles on a scrubbable timeline
- Save trim as a **copy** or **overwrite** the original
- Frame-accurate previews

### Cloud Uploading

Upload clips directly to your own **Cloudflare R2** bucket. Klyppd generates a small HTML embed page alongside the video file with proper OpenGraph and Twitter Card meta tags, so links unfurl into rich Discord / Twitter previews automatically.

- Custom domains
- Temporary or permanent uploads
- Short, branded share links
- Auto-expiring temp clips via R2 lifecycle rules

```txt
https://clip.example.com/t/abc123   # temporary
https://clip.example.com/p/abc123   # permanent
```

> Additional cloud providers are planned for future releases.

### Customization

Klyppd is designed to be tweakable. You can configure:

- **Themes** via plain CSS (`~/.config/klyppd/theme.css`)
- **Buffer duration**, FPS, codec, container, audio source
- **Hotkeys** through your window manager
- **Clip save location**

More advanced customization options are planned over time.

---

## How It Works

Klyppd acts as a frontend for `gpu-screen-recorder` while layering on:

- A graphical interface
- Clip management with thumbnails and metadata
- Upload integration
- Trimming tools
- Custom workflows

Hotkey integrations communicate with the running app through a Unix socket under the user runtime directory (`$XDG_RUNTIME_DIR/klyppd/klyppd.sock`, with a temp fallback), so triggers always read live from the app's settings — no shell scripts to keep in sync.

---

## Requirements

### Arch Linux

```bash
sudo pacman -S webkit2gtk-4.1 gpu-screen-recorder ffmpeg \
               gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav
```

### Ubuntu / Debian

```bash
sudo apt install libwebkit2gtk-4.1-dev gpu-screen-recorder ffmpeg \
                 gstreamer1.0-plugins-{base,good,bad} gstreamer1.0-libav
```

You also need a Rust toolchain (≥ 1.77) and Node.js (≥ 18) to build from source.

---

## Installation

### AUR (Arch Linux)

```bash
# Recommended — prebuilt binary, installs in seconds
yay -S klyppd-bin

# Or build from latest main (takes a few minutes)
yay -S klyppd-git
```

### From source

```bash
git clone https://github.com/brookerslyn/klyppd
cd klyppd
npm install
npm run tauri build -- --bundles deb
```

This produces a `.deb` at `src-tauri/target/release/bundle/deb/klyppd_0.3.2_amd64.deb`.

#### Install on Debian / Ubuntu

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/klyppd_0.3.2_amd64.deb
sudo apt -f install
```

#### Run without installing

```bash
./src-tauri/target/release/klyppd
```

### Development

```bash
npm run tauri dev
```

### Faster builds (recommended)

Install [mold](https://github.com/rui314/mold) for ~50% faster linking:

```bash
sudo pacman -S mold      # Arch
sudo apt install mold    # Debian/Ubuntu
```

The repo's `src-tauri/.cargo/config.toml` is preconfigured to use it.

---

## Configuration

Settings live in `~/.config/klyppd/settings.json` and are configured through the Settings tab in-app:

| Setting          | Description                          |
| ---------------- | ------------------------------------ |
| Frame rate       | Recording FPS                        |
| Codec            | h264 / hevc / av1                    |
| Container        | mp4 / mkv / webm                     |
| Audio source     | gpu-screen-recorder audio specifier  |
| Clip directory   | Where clips are saved locally        |
| Buffer duration  | Length of the replay buffer          |

### Cloudflare R2

In **Settings → Cloudflare R2**, fill in:

| Field          | Value                                                   |
| -------------- | ------------------------------------------------------- |
| Endpoint       | `https://<account_id>.r2.cloudflarestorage.com`         |
| Bucket         | your bucket name                                        |
| Access Key     | from R2 → Manage API Tokens                             |
| Secret Key     | from R2 → Manage API Tokens                             |
| Custom Domain  | `https://cdn.yourdomain.com`                            |
| Temp expiry    | matches your bucket's lifecycle rule on the `t/` prefix |

Add a lifecycle rule on the `t/` prefix in your bucket settings to auto-delete temporary clips.

### Hotkeys

Klyppd uses **evdev** (`/dev/input/`) for global hotkeys — they work on **any compositor** (Hyprland, GNOME, KDE, Sway, X11) without any window manager configuration.

#### Setup

Add your user to the `input` group (one-time, requires logout):

```bash
sudo usermod -aG input $USER
```

Then configure hotkeys in **Settings → Hotkeys** inside the app. Any key combo works:

| Action | Default | Examples |
|---|---|---|
| Save replay | `Alt+R` | `F9`, `Super+C`, `Ctrl+Shift+S` |
| Toggle recording | `Alt+Shift+R` | `F10`, `Super+R` |
| Toggle buffer | `Alt+F8` | `F8`, `Super+B` |

Hotkeys work even when a fullscreen game has focus.

#### Fallback: compositor keybinds (optional)

If you prefer not to use the `input` group, you can bind through your compositor instead. These talk to the running app via the same Unix socket:

```ini
# hyprland.conf
bind = ALT,       R, exec, klyppd --cmd save-replay
bind = ALT SHIFT, R, exec, klyppd --cmd toggle-recording
bind = ALT,      F8, exec, klyppd --cmd toggle-buffer
```

```lua
-- hyprland-lua
hl.bind("ALT + R",         hl.dsp.exec_cmd("klyppd --cmd save-replay"))
hl.bind("ALT + SHIFT + R", hl.dsp.exec_cmd("klyppd --cmd toggle-recording"))
hl.bind("ALT + F8",        hl.dsp.exec_cmd("klyppd --cmd toggle-buffer"))
```

KDE / GNOME / other compositors: use any keybind manager that can run a shell command.

### Theming

Drop a `~/.config/klyppd/theme.css` file to override any of the CSS variables. The app also honors the `theme_path` setting if you want to point at another file:

```css
:root {
  --bg-deepest: #0b0f12;
  --bg-base: #101418;
  --bg-elev-1: #181c20;
  --bg-elev-2: #1c2024;
  --bg-elev-3: #272a2e;
  --text: #e0e2e8;
  --text-dim: #c2c7cf;
  --text-muted: #8c9198;
  --accent: #9accfa;
  --border: #1c2024;
  --border-strong: #36393e;
}
```

---

## Project Status

Klyppd is under **active development**.

If you wish to help with the development of Klyppd you can contribute and help with the following:

## Roadmap

- [ ] Additional cloud providers (S3, MinIO, custom HTTP)
- [ ] Better UI customization
- [ ] Improved clip management (tags, folders, filters)
- [ ] In-app hotkey configuration
- [ ] Cross-platform support
- [ ] Automatic updates
- [ ] Better onboarding / setup experience
- [ ] AUR + AppImage releases

---

## Contributing

Contributions are welcome. Whether you want to:

- Improve the UI
- Add features
- Fix bugs
- Package Klyppd for other distributions
- Improve documentation

…feel free to open a pull request or fork the project.

---

## Support

If Klyppd saved you a few clicks (or a few hundred), you can support the project here:

[![Ko-fi](https://img.shields.io/badge/Ko--fi-support-FF5E5B?logo=kofi&logoColor=white)](https://ko-fi.com/brookerslyn)
[![PayPal](https://img.shields.io/badge/PayPal-donate-00457C?logo=paypal&logoColor=white)](https://www.paypal.com/paypalme/brookerslyn)

If you can't donate, **starring the repository** helps a ton too.

---

## License

[MIT](LICENSE)
