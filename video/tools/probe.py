"""Checks a delivered film: the container and stream facts, loudness, fast start, and its captions.

    .venv\\Scripts\\python.exe -I tools\\probe.py <film> --video file.mp4 --vtt file.vtt [--voice af_heart] [--report out.json]

Captions are checked against the timeline (every cue sits inside one scene) and against the audio (the voice
is actually speaking for most of every cue), because an agent cannot listen.
"""
import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

VIDEO = Path(__file__).resolve().parent.parent
FFMPEG = os.environ.get("PGR_FFMPEG") or str(VIDEO / "bin" / "ffmpeg.exe")


def atoms(path: Path):
    """Top-level MP4 atoms in file order."""
    out, pos, size = [], 0, path.stat().st_size
    with open(path, "rb") as f:
        while pos < size:
            f.seek(pos)
            head = f.read(8)
            if len(head) < 8:
                break
            n = int.from_bytes(head[:4], "big")
            kind = head[4:8].decode("latin1")
            if n == 1:
                n = int.from_bytes(f.read(8), "big")
            out.append(kind)
            if n == 0:
                break
            pos += n
    return out


def last_json(text: str) -> dict:
    start = text.rindex("{")
    return json.loads(text[start:text.index("}", start) + 1])


def loudness(path: Path):
    p = subprocess.run([FFMPEG, "-hide_banner", "-nostats", "-i", str(path), "-vn", "-af", "loudnorm=I=-16:TP=-1.5:print_format=json", "-f", "null", "-"],
                       capture_output=True, text=True)
    j = last_json(p.stderr)
    return {"integrated_lufs": float(j["input_i"]), "true_peak_db": float(j["input_tp"]), "range_lu": float(j["input_lra"])}


def parse_vtt(path: Path):
    cues = []
    blocks = re.split(r"\n\s*\n", path.read_text(encoding="utf-8").strip())
    for b in blocks[1:]:
        lines = b.strip().splitlines()
        m = re.match(r"(\d+):(\d\d):(\d\d\.\d+) --> (\d+):(\d\d):(\d\d\.\d+)", lines[1])
        a = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
        z = int(m[4]) * 3600 + int(m[5]) * 60 + float(m[6])
        cues.append({"start": a, "end": z, "lines": lines[2:]})
    return cues


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("film")
    ap.add_argument("--video", required=True)
    ap.add_argument("--vtt")
    ap.add_argument("--voice", default="af_heart")
    ap.add_argument("--report")
    a = ap.parse_args()
    import av
    path = Path(a.video)
    c = av.open(str(path))
    v = c.streams.video[0]
    au = c.streams.audio[0]
    cc = v.codec_context
    info = {
        "file": path.name, "bytes": path.stat().st_size, "mb": round(path.stat().st_size / 1e6, 2),
        "seconds": round(float(c.duration) / av.time_base, 3),
        "video": {"codec": cc.name, "profile": cc.profile, "width": cc.width, "height": cc.height, "pix_fmt": cc.pix_fmt,
                  "fps": float(v.average_rate), "frames": v.frames, "bit_rate_kbps": round((cc.bit_rate or 0) / 1000) or None},
        "audio": {"codec": au.codec_context.name, "channels": au.codec_context.layout.nb_channels, "sample_rate": au.codec_context.sample_rate,
                  "bit_rate_kbps": round((au.codec_context.bit_rate or 0) / 1000) or None},
        "overall_kbps": round(path.stat().st_size * 8 / float(c.duration) * av.time_base / 1000),
    }
    c.close()
    order = atoms(path)
    info["faststart"] = "moov" in order and "mdat" in order and order.index("moov") < order.index("mdat")
    info["loudness"] = loudness(path)
    problems = []
    if info["video"]["profile"] not in ("High", "High 4:4:4 Predictive"):
        problems.append("profile is not High: " + str(info["video"]["profile"]))
    if info["video"]["pix_fmt"] != "yuv420p":
        problems.append("pix_fmt is not yuv420p")
    if info["audio"]["channels"] != 2 or info["audio"]["codec"] != "aac":
        problems.append("audio is not AAC stereo")
    if not info["faststart"]:
        problems.append("moov atom is not before mdat")
    if info["loudness"]["true_peak_db"] > -1.0:
        problems.append("true peak above -1 dB")
    if abs(info["loudness"]["integrated_lufs"] + 16) > 1.5:
        problems.append("integrated loudness is more than 1.5 LU from -16")

    if a.vtt:
        tl = json.loads((VIDEO / "build" / f"{a.film}.timeline.json").read_text(encoding="utf-8"))
        cues = parse_vtt(Path(a.vtt))
        import soundfile as sf
        x, sr = sf.read(str(VIDEO / "build" / f"{a.film}.{a.voice}.voice.wav"), dtype="float32")
        hop = int(sr * 0.01)
        env = np.sqrt((x[: len(x) // hop * hop].reshape(-1, hop) ** 2).mean(axis=1)) > 0.004
        cap = {"cues": len(cues), "max_line_chars": 0, "max_lines": 0, "outside_scene": [], "low_speech_overlap": [], "overlaps": [], "short": [], "fast": []}
        prev_end = 0.0
        for k, q in enumerate(cues):
            cap["max_lines"] = max(cap["max_lines"], len(q["lines"]))
            cap["max_line_chars"] = max([cap["max_line_chars"]] + [len(l) for l in q["lines"]])
            if q["start"] < prev_end - 1e-6:
                cap["overlaps"].append(k + 1)
            span = q["end"] - q["start"]
            chars = len(" ".join(q["lines"]))
            if span < 0.9:
                cap["short"].append({"cue": k + 1, "seconds": round(span, 2), "text": " ".join(q["lines"])})
            if chars / max(span, 0.01) > 21:
                cap["fast"].append({"cue": k + 1, "chars_per_second": round(chars / span, 1), "text": " ".join(q["lines"])})
            prev_end = q["end"]
            scene = [s for s in tl["scenes"] if s["start"] - 1e-6 <= q["start"] and q["end"] <= s["start"] + s["dur"] + 1e-6]
            if not scene:
                cap["outside_scene"].append(k + 1)
            a0, a1 = int(q["start"] * 100), max(int(q["end"] * 100), int(q["start"] * 100) + 1)
            frac = float(env[a0:a1].mean()) if a1 <= len(env) else 0.0
            if frac < 0.6:
                cap["low_speech_overlap"].append({"cue": k + 1, "speech_share": round(frac, 2), "text": " ".join(q["lines"])})
        info["captions"] = cap
        if cap["max_lines"] > 2 or cap["max_line_chars"] > 42:
            problems.append("a caption has more than 2 lines or 42 characters per line")
        if cap["outside_scene"] or cap["overlaps"]:
            problems.append("captions outside their scene or overlapping")
        if cap["short"]:
            problems.append("captions on screen for under 0.9 s: " + json.dumps(cap["short"]))
        if cap["fast"]:
            problems.append("captions read faster than 21 characters a second: " + json.dumps(cap["fast"]))
        if cap["low_speech_overlap"]:
            problems.append("some captions are mostly over silence: " + json.dumps(cap["low_speech_overlap"]))
        if cues and cues[-1]["end"] > info["seconds"] + 0.05:
            problems.append("a caption ends after the film")
    info["problems"] = problems
    if a.report:
        Path(a.report).write_text(json.dumps(info, indent=1), encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    print(json.dumps(info, indent=1))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
