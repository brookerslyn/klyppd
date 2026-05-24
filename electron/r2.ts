import {
  S3Client,
  PutObjectCommand,
  DeleteObjectCommand,
  ListObjectsV2Command,
} from "@aws-sdk/client-s3";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { v4 as uuidv4 } from "uuid";
import type { Clip } from "./db.js";
import type { AppSettings } from "./settings.js";
import { hasAacAudio, getVideoResolution } from "./editor.js";

function shortId(): string {
  return uuidv4().slice(0, 8);
}

function getClient(s: AppSettings): S3Client {
  return new S3Client({
    region: "auto",
    endpoint: s.r2_endpoint,
    credentials: {
      accessKeyId: s.r2_access_key,
      secretAccessKey: s.r2_secret_key,
    },
    forcePathStyle: true,
  });
}

function fmtBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1048576) return `${Math.floor(b / 1024)} KB`;
  if (b < 1073741824) return `${(b / 1048576).toFixed(1)} MB`;
  return `${(b / 1073741824).toFixed(2)} GB`;
}

function splitFilename(filename: string): { title: string; date: string } {
  const stem = filename.replace(/\.[^.]+$/, "");
  const match = stem.match(/_(\d{4}-\d{2}-\d{2})$/);
  if (match) {
    return { title: stem.slice(0, match.index!), date: match[1] };
  }
  return { title: stem, date: "" };
}

function prettyDate(date: string): string {
  if (date.length < 10) return date;
  const months = [
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
  ];
  const month = parseInt(date.slice(5, 7), 10);
  const day = parseInt(date.slice(8, 10), 10);
  const year = date.slice(0, 4);
  if (month >= 1 && month <= 12) {
    return `${months[month - 1]} ${day}, ${year}`;
  }
  return date;
}

function buildEmbed(
  video: string,
  thumb: string,
  title: string,
  date: string,
  size: string,
  w: number,
  h: number
): string {
  const site = `Clipped with Klyppd \u00b7 ${size}`;
  const fullTitle = date ? `${title} (${date})` : title;
  const desc = "Clipped with Klyppd";
  const repo = "https://github.com/brookerslyn/klyppd";

  return `<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<title>${fullTitle} \u2014 klyppd</title>
<meta property="og:type" content="video.other">
<meta property="og:title" content="${fullTitle}">
<meta property="og:description" content="${desc}">
<meta property="og:site_name" content="${site}">
<meta property="og:image" content="${thumb}">
<meta property="og:image:width" content="${w}">
<meta property="og:image:height" content="${h}">
<meta property="og:video" content="${video}">
<meta property="og:video:secure_url" content="${video}">
<meta property="og:video:type" content="video/mp4">
<meta property="og:video:width" content="${w}">
<meta property="og:video:height" content="${h}">
<meta name="twitter:card" content="player">
<meta name="twitter:title" content="${fullTitle}">
<meta name="twitter:description" content="${desc}">
<meta name="twitter:image" content="${thumb}">
<meta name="twitter:player" content="${video}">
<meta name="twitter:player:stream" content="${video}">
<meta name="twitter:player:stream:content_type" content="video/mp4">
<meta name="twitter:player:width" content="${w}">
<meta name="twitter:player:height" content="${h}">
<meta name="theme-color" content="#9accfa">
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{background:#0b0f12;color:#e0e2e8;font-family:-apple-system,'Inter','Segoe UI',sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;min-height:100vh;padding:24px}
.wrap{max-width:1100px;width:100%}
video{width:100%;border-radius:8px;background:#000;display:block;box-shadow:0 8px 30px rgba(0,0,0,.5)}
.title{margin-top:16px;font-size:22px;font-weight:600;letter-spacing:-.01em}
.sub{margin-top:6px;font-size:13px;color:#8c9198}
.foot{margin-top:18px;padding-top:14px;border-top:1px solid #1c2024;font-size:11px;color:#42474e;display:flex;justify-content:space-between}
a{color:#9accfa;text-decoration:none;font-size:11px}
a:hover{text-decoration:underline}
</style></head>
<body>
<div class="wrap">
<video src="${video}" controls autoplay playsinline poster="${thumb}"></video>
<div class="title">${fullTitle}</div>
<div class="sub">${desc} \u00b7 ${size}</div>
<div class="foot"><span>${date}</span><a href="${repo}">Klyppd</a></div>
</div>
</body></html>`;
}

function ensureAac(filePath: string): string {
  if (hasAacAudio(filePath)) return filePath;

  const tmp = `${filePath}.upload.mp4`;
  try {
    execFileSync("ffmpeg", [
      "-y",
      "-i",
      filePath,
      "-c:v",
      "copy",
      "-c:a",
      "aac",
      "-b:a",
      "160k",
      tmp,
    ]);
    return tmp;
  } catch {
    return filePath;
  }
}

export async function upload(
  s: AppSettings,
  clip: Clip,
  permanent: boolean
): Promise<string> {
  const client = getClient(s);
  const ext = path.extname(clip.path).slice(1) || "mp4";
  const id = shortId();
  const prefix = permanent ? "p" : "t";
  const videoKey = `${prefix}/${id}.${ext}`;
  const thumbKey = `${prefix}/${id}.jpg`;
  const htmlKey = `${prefix}/${id}`;
  const domain = s.r2_custom_domain.replace(/\/$/, "");

  const size = fs.existsSync(clip.path)
    ? fs.statSync(clip.path).size
    : 0;
  const { title, date } = splitFilename(clip.filename);

  const uploadPath = ensureAac(clip.path);

  await client.send(
    new PutObjectCommand({
      Bucket: s.r2_bucket,
      Key: videoKey,
      Body: fs.readFileSync(uploadPath),
      ContentType: `video/${ext}`,
    })
  );

  if (uploadPath !== clip.path) {
    try {
      fs.unlinkSync(uploadPath);
    } catch {}
  }

  let thumbUrl = "";
  if (clip.thumbnail_path && fs.existsSync(clip.thumbnail_path)) {
    try {
      await client.send(
        new PutObjectCommand({
          Bucket: s.r2_bucket,
          Key: thumbKey,
          Body: fs.readFileSync(clip.thumbnail_path),
          ContentType: "image/jpeg",
        })
      );
      thumbUrl = `${domain}/${thumbKey}`;
    } catch {}
  }

  const videoUrl = `${domain}/${videoKey}`;
  const res = getVideoResolution(clip.path);
  const w = res?.width ?? 1280;
  const h = res?.height ?? 720;
  const html = buildEmbed(
    videoUrl,
    thumbUrl,
    title,
    prettyDate(date),
    fmtBytes(size),
    w,
    h
  );

  await client.send(
    new PutObjectCommand({
      Bucket: s.r2_bucket,
      Key: htmlKey,
      Body: Buffer.from(html),
      ContentType: "text/html; charset=utf-8",
    })
  );

  return `${domain}/${htmlKey}`;
}

function stripVideoExt(key: string): string {
  for (const ext of [".html", ".mp4", ".mkv", ".webm", ".jpg"]) {
    if (key.endsWith(ext)) return key.slice(0, -ext.length);
  }
  return key;
}

export async function deleteFromR2(
  s: AppSettings,
  key: string
): Promise<void> {
  const client = getClient(s);
  const base = stripVideoExt(key);
  const variants = [
    base,
    `${base}.mp4`,
    `${base}.mkv`,
    `${base}.webm`,
    `${base}.jpg`,
  ];
  for (const v of variants) {
    try {
      await client.send(
        new DeleteObjectCommand({ Bucket: s.r2_bucket, Key: v })
      );
    } catch {}
  }
}

export async function storageUsage(
  s: AppSettings,
  prefix: string
): Promise<number> {
  const client = getClient(s);
  let total = 0;
  let continuationToken: string | undefined;

  do {
    const resp = await client.send(
      new ListObjectsV2Command({
        Bucket: s.r2_bucket,
        Prefix: prefix,
        ContinuationToken: continuationToken,
      })
    );
    for (const obj of resp.Contents ?? []) {
      total += obj.Size ?? 0;
    }
    continuationToken = resp.IsTruncated
      ? resp.NextContinuationToken
      : undefined;
  } while (continuationToken);

  return total;
}
