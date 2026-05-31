use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

type Error = Box<dyn std::error::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTrackMeta {
    pub key: String,
    pub label: String,
    pub source: String,
    pub isolated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipAudioTrack {
    pub index: usize,
    pub key: String,
    pub label: String,
    pub isolated: bool,
}

pub fn trim(
    input: &str,
    output: &str,
    start: f64,
    end: f64,
    enabled_audio_tracks: Option<&[usize]>,
) -> Result<String, Error> {
    // NOTE: -c:v copy is lossless but can leave a few black frames at the start
    // if the cut point isn't on a keyframe. Tried `-avoid_negative_ts make_zero`
    // but it didn't help consistently. Living with it for now.
    //
    // TODO: add a "precise" mode that re-encodes the first GOP only
    // (ffmpeg -ss before -i is keyframe-aligned, -ss after -i is frame-accurate but slow)
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-ss",
        &format!("{start:.3}"),
        "-to",
        &format!("{end:.3}"),
        "-i",
        input,
        "-map",
        "0:v:0",
        "-c:v",
        "copy",
    ]);

    append_audio_mix_args(&mut cmd, input, enabled_audio_tracks);

    let ok = cmd.arg(output).status()?.success();
    if ok { Ok(output.into()) } else { Err("ffmpeg trim failed".into()) }
}

pub fn render_browser_copy(
    input: &str,
    output: &str,
    enabled_audio_tracks: Option<&[usize]>,
) -> Result<String, Error> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i", input, "-map", "0:v:0", "-c:v", "copy"]);
    append_audio_mix_args(&mut cmd, input, enabled_audio_tracks);
    let ok = cmd.args(["-movflags", "+faststart", output]).status()?.success();
    if ok { Ok(output.into()) } else { Err("ffmpeg render failed".into()) }
}

fn append_audio_mix_args(cmd: &mut Command, input: &str, enabled_audio_tracks: Option<&[usize]>) {
    let audio_streams = audio_stream_count(input);
    let tracks: Vec<usize> = enabled_audio_tracks
        .map(|indices| {
            indices
                .iter()
                .copied()
                .filter(|index| *index < audio_streams)
                .collect()
        })
        .unwrap_or_else(|| (0..audio_streams).collect());

    if tracks.is_empty() {
        cmd.arg("-an");
    } else if tracks.len() == 1 {
        cmd.args([
            "-map",
            &format!("0:a:{}", tracks[0]),
            "-c:a",
            "aac",
            "-b:a",
            "160k",
        ]);
    } else {
        let filter = tracks
            .iter()
            .map(|i| format!("[0:a:{i}]"))
            .collect::<Vec<_>>()
            .join("");
        cmd.args([
            "-filter_complex",
            &format!("{filter}amix=inputs={}:normalize=0[aout]", tracks.len()),
            "-map",
            "[aout]",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
        ]);
    }
}

// Experimented with generating a thumbnail strip for the timeline scrubber:
// fn generate_timeline_strip(input: &str, output: &str, count: u32) -> Result<(), Error> {
//     let filter = format!("select='not(mod(n,{}))',scale=160:-1,tile={}x1", count, count);
//     Command::new("ffmpeg")
//         .args(["-y", "-i", input, "-vf", &filter, "-frames:v", "1", output])
//         .status()?;
//     Ok(())
// }
// ^ shelved — adds 2-3s latency opening the editor, not worth it until we can do it async

pub fn crop(input: &str, output: &str, x: u32, y: u32, w: u32, h: u32) -> Result<String, Error> {
    let filter = format!("crop={w}:{h}:{x}:{y}");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-i", input, "-vf", &filter, "-c:v", "libx264", "-c:a", "copy", output])
        .status()?
        .success();
    if ok { Ok(output.into()) } else { Err("ffmpeg crop failed".into()) }
}

pub fn generate_thumbnail(input: &str, output: &str) -> Result<(), Error> {
    let ok = Command::new("ffmpeg")
        .args(["-y", "-ss", "1", "-i", input, "-vframes", "1", "-vf", "scale=320:-1", output])
        .status()?
        .success();
    if ok { Ok(()) } else { Err("thumbnail generation failed".into()) }
}

pub fn get_duration(input: &str) -> Result<f64, Error> {
    let out = Command::new("ffprobe")
        .args(["-v", "quiet", "-show_entries", "format=duration", "-of", "csv=p=0", input])
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse()?)
}

pub fn audio_stream_count(input: &str) -> usize {
    Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
            input,
        ])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0)
}

pub fn audio_tracks_sidecar_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("clip");
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{filename}.klyppd-audio.json"))
}

pub fn write_audio_tracks_sidecar(path: &str, tracks: &[AudioTrackMeta]) -> Result<(), Error> {
    let sidecar = audio_tracks_sidecar_path(path);
    if tracks.is_empty() {
        std::fs::remove_file(sidecar).ok();
        return Ok(());
    }

    std::fs::write(sidecar, serde_json::to_vec_pretty(tracks)?)?;
    Ok(())
}

pub fn rename_audio_tracks_sidecar(src: &str, dst: &str) -> Result<(), Error> {
    let src_sidecar = audio_tracks_sidecar_path(src);
    if !src_sidecar.exists() {
        return Ok(());
    }

    let dst_sidecar = audio_tracks_sidecar_path(dst);
    std::fs::rename(src_sidecar, dst_sidecar)?;
    Ok(())
}

pub fn remove_audio_tracks_sidecar(path: &str) -> Result<(), Error> {
    std::fs::remove_file(audio_tracks_sidecar_path(path)).ok();
    Ok(())
}

pub fn get_audio_tracks(path: &str) -> Vec<ClipAudioTrack> {
    let count = audio_stream_count(path);
    if count == 0 {
        return Vec::new();
    }

    let meta = std::fs::read(audio_tracks_sidecar_path(path))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<AudioTrackMeta>>(&bytes).ok())
        .unwrap_or_default();

    if meta.is_empty() {
        return (0..count)
            .map(|index| ClipAudioTrack {
                index,
                key: format!("track_{index}"),
                label: if count == 1 {
                    "audio".into()
                } else {
                    format!("track {}", index + 1)
                },
                isolated: count == 1,
            })
            .collect();
    }

    meta.into_iter()
        .enumerate()
        .take(count)
        .map(|(index, track)| ClipAudioTrack {
            index,
            key: track.key,
            label: track.label,
            isolated: track.isolated,
        })
        .collect()
}
