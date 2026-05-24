import { execFileSync, execSync } from "node:child_process";

export function trim(
  input: string,
  output: string,
  start: number,
  end: number
): string {
  try {
    execFileSync("ffmpeg", [
      "-y",
      "-ss",
      start.toFixed(3),
      "-to",
      end.toFixed(3),
      "-i",
      input,
      "-c:v",
      "copy",
      "-c:a",
      "aac",
      "-b:a",
      "160k",
      output,
    ]);
    return output;
  } catch {
    throw new Error("ffmpeg trim failed");
  }
}

export function crop(
  input: string,
  output: string,
  x: number,
  y: number,
  w: number,
  h: number
): string {
  const filter = `crop=${w}:${h}:${x}:${y}`;
  try {
    execFileSync("ffmpeg", [
      "-y",
      "-i",
      input,
      "-vf",
      filter,
      "-c:v",
      "libx264",
      "-c:a",
      "copy",
      output,
    ]);
    return output;
  } catch {
    throw new Error("ffmpeg crop failed");
  }
}

export function generateThumbnail(input: string, output: string): void {
  try {
    execFileSync("ffmpeg", [
      "-y",
      "-ss",
      "1",
      "-i",
      input,
      "-vframes",
      "1",
      "-vf",
      "scale=320:-1",
      output,
    ]);
  } catch {
    throw new Error("thumbnail generation failed");
  }
}

export function getDuration(input: string): number {
  try {
    const out = execFileSync("ffprobe", [
      "-v",
      "quiet",
      "-show_entries",
      "format=duration",
      "-of",
      "csv=p=0",
      input,
    ]).toString();
    return parseFloat(out.trim()) || 0;
  } catch {
    return 0;
  }
}

export function hasAacAudio(filePath: string): boolean {
  try {
    const out = execFileSync("ffprobe", [
      "-v",
      "quiet",
      "-select_streams",
      "a:0",
      "-show_entries",
      "stream=codec_name",
      "-of",
      "csv=p=0",
      filePath,
    ]).toString();
    return out.trim() === "aac";
  } catch {
    return false;
  }
}

export function getVideoResolution(
  filePath: string
): { width: number; height: number } | null {
  try {
    const out = execFileSync("ffprobe", [
      "-v",
      "quiet",
      "-select_streams",
      "v:0",
      "-show_entries",
      "stream=width,height",
      "-of",
      "csv=p=0:s=x",
      filePath,
    ]).toString();
    const parts = out.trim().split("x");
    if (parts.length === 2) {
      const width = parseInt(parts[0], 10);
      const height = parseInt(parts[1], 10);
      if (!isNaN(width) && !isNaN(height)) return { width, height };
    }
    return null;
  } catch {
    return null;
  }
}
