import { ChildProcess, spawn } from "node:child_process";
import { AppSettings } from "./settings.js";

export interface RecordingState {
  replay_buffer_active: boolean;
  recording_active: boolean;
}

function detectCaptureTarget(): string {
  const isWayland =
    !!process.env.WAYLAND_DISPLAY ||
    process.env.XDG_SESSION_TYPE === "wayland";

  if (isWayland) return "portal";

  try {
    const { execSync } = require("node:child_process");
    const out = execSync("gpu-screen-recorder --list-monitors", {
      encoding: "utf-8",
      timeout: 5000,
    });
    const line = out
      .split("\n")
      .find((l: string) => l.trim() && !l.startsWith(" "));
    if (line) {
      const monitor = line.split(/\s+/)[0];
      if (monitor) return monitor;
    }
  } catch {}

  return "screen";
}

function buildBaseArgs(s: AppSettings): string[] {
  const target = detectCaptureTarget();
  const args = [
    "-w",
    target,
    "-f",
    s.fps.toString(),
    "-k",
    s.codec,
    "-ac",
    s.audio_codec,
  ];
  if (s.audio_source) {
    args.push("-a", s.audio_source);
  }
  return args;
}

export class Recorder {
  private replay: ChildProcess | null = null;
  private recording: ChildProcess | null = null;

  getState(): RecordingState {
    return {
      replay_buffer_active: this.replay !== null && !this.replay.killed,
      recording_active: this.recording !== null && !this.recording.killed,
    };
  }

  startReplayBuffer(s: AppSettings): void {
    if (this.replay && !this.replay.killed)
      throw new Error("replay buffer already running");

    const args = [
      ...buildBaseArgs(s),
      "-r",
      s.buffer_seconds.toString(),
      "-c",
      s.container,
      "-o",
      s.clips_directory,
    ];

    this.replay = spawn("gpu-screen-recorder", args, {
      stdio: "ignore",
      detached: false,
    });
    this.replay.on("exit", () => {
      this.replay = null;
    });
  }

  stopReplayBuffer(): void {
    if (!this.replay || this.replay.killed)
      throw new Error("no replay buffer running");
    this.replay.kill("SIGINT");
    this.replay = null;
  }

  saveReplay(): void {
    if (!this.replay || this.replay.killed)
      throw new Error("no replay buffer running");
    this.replay.kill("SIGUSR1");
  }

  startRecording(s: AppSettings): void {
    if (this.recording && !this.recording.killed)
      throw new Error("already recording");

    const stamp = new Date()
      .toISOString()
      .replace(/[:-]/g, "")
      .replace("T", "_")
      .slice(0, 15);
    const output = `${s.clips_directory}/${stamp}.${s.container}`;

    const args = [...buildBaseArgs(s), "-o", output];

    this.recording = spawn("gpu-screen-recorder", args, {
      stdio: "ignore",
      detached: false,
    });
    this.recording.on("exit", () => {
      this.recording = null;
    });
  }

  stopRecording(): string {
    if (!this.recording || this.recording.killed)
      throw new Error("not recording");
    this.recording.kill("SIGINT");
    this.recording = null;
    return "recording saved";
  }

  cleanup(): void {
    if (this.replay && !this.replay.killed) {
      this.replay.kill("SIGINT");
    }
    if (this.recording && !this.recording.killed) {
      this.recording.kill("SIGINT");
    }
  }
}
