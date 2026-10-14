"""Voice step for one film: narration -> per-cue speech -> timeline -> captions -> mastered audio.

    .venv\\Scripts\\python.exe -I tools\\voice.py explainer [--primary af_heart] [--voices af_heart,am_michael]

Everything runs on this machine: the Kokoro speech model through onnxruntime, numpy for the mix, FFmpeg for
loudness. The spoken numbers are filled from build/facts.json (written by tools/facts.mjs from the site's data),
written out as words. The voice sets the scene lengths: each cue is synthesised alone, measured, and the
timeline is built from the measurements (nothing is hand-timed).

A recorded voice can replace the synthetic one: put one audio file per scene in
build/voice/<film>/recorded/<scene id>.wav (or .mp3, .m4a, .flac). Scene lengths then follow those files and the
cues inside a scene are spread over the file by their number of characters.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np

VIDEO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))
FFMPEG = os.environ.get("PGR_FFMPEG") or str(VIDEO / "bin" / "ffmpeg.exe")
MODEL = VIDEO / "models" / "kokoro-v1.0.onnx"
VOICES = VIDEO / "models" / "voices-v1.0.bin"
SR = 24000          # the speech model's native rate
OUT_SR = 48000      # the delivered audio
BED_BELOW_DB = 14   # music bed under the voice, integrated loudness (the brief asks for at least 14; 18 left the bed inaudible on a phone)


def run(cmd, **kw):
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode:
        raise SystemExit(f"command failed: {' '.join(map(str, cmd))}\n{p.stderr[-1500:]}")
    return p


# ---------------------------------------------------------------- text

SMALL = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]


def fill(template: str, table: dict, capitalise_first: bool) -> str:
    """Replace {key} with the value in table. A value at the very start of a sentence gets a capital; in a
    caption a single digit that starts a sentence is written out ("Five turns.")."""
    def sub(m):
        key = m.group(1)
        if key not in table:
            raise SystemExit(f"script uses {{{key}}} but facts.json has no such number")
        value = table[key]
        start = m.start() == 0 or template[:m.start()].rstrip().endswith((".", "!", "?"))
        if start and not capitalise_first and value.isdigit() and len(value) == 1:
            value = SMALL[int(value)]
            capitalise = True
        else:
            capitalise = capitalise_first
        return value[:1].upper() + value[1:] if (capitalise and start) else value
    return re.sub(r"\{(\w+)\}", sub, template)


def load_script(film: str, facts: dict):
    script = json.loads((VIDEO / "script" / f"{film}.json").read_text(encoding="utf-8"))
    for scene in script["scenes"]:
        for cue in scene["cues"]:
            cue["caption"] = fill(cue["text"], facts["cap"], False)
            spoken = cue.get("say") or cue["text"]
            cue["spoken"] = fill(spoken, facts["say"], True)
    return script


def split_caption(text: str, max_line=42, max_lines=2):
    """Split a caption into pieces of at most two lines of 42 characters. Returns [(lines, weight)]."""
    words = text.split()
    pieces, cur = [], []
    def wrap(ws):
        lines, line = [], ""
        for w in ws:
            if len(line) + len(w) + (1 if line else 0) <= max_line:
                line = (line + " " + w).strip()
            else:
                lines.append(line)
                line = w
        lines.append(line)
        return lines
    for w in words:
        trial = wrap(cur + [w])
        if len(trial) > max_lines or any(len(x) > max_line for x in trial):
            pieces.append(cur)
            cur = [w]
        else:
            cur.append(w)
    if cur:
        pieces.append(cur)
    # a piece that needs two lines is broken so the lines are nearly as long as each other, after punctuation when
    # that costs little: no caption ends on a lone word
    def balanced(ws):
        if len(" ".join(ws)) <= max_line:
            return [" ".join(ws)]
        best = None
        for k in range(1, len(ws)):
            a, b = " ".join(ws[:k]), " ".join(ws[k:])
            if len(a) <= max_line and len(b) <= max_line:
                score = max(len(a), len(b)) - (4 if a[-1] in ",:;." else 0)
                if best is None or score < best[0]:
                    best = (score, [a, b])
        return best[1] if best else wrap(ws)
    def fits(ws):
        return len(wrap(ws)) <= max_lines and all(len(x) <= max_line for x in wrap(ws))
    # a last piece of under 18 characters takes words from the piece before it while both still fit and it stays the shorter one:
    # a caption of two words that flashes by is worse than two even ones
    size = lambda ws: len(" ".join(ws))
    while len(pieces) > 1 and size(pieces[-1]) < 18 and len(pieces[-2]) > 2:
        a, b = pieces[-2][:-1], [pieces[-2][-1]] + pieces[-1]
        if not (fits(a) and fits(b)) or size(b) > size(a):
            break
        pieces[-2], pieces[-1] = a, b
    out = []
    for p in pieces:
        out.append((balanced(p), sum(len(w) for w in p) + len(p)))
    return out


# ---------------------------------------------------------------- audio

def trim(x: np.ndarray, head=0.03, tail=0.12, floor_db=-46.0):
    if x.size == 0:
        return x
    win = int(SR * 0.005)
    env = np.convolve(np.abs(x), np.ones(win) / win, mode="same")
    live = np.where(env > 10 ** (floor_db / 20))[0]
    if live.size == 0:
        return x
    a = max(0, live[0] - int(head * SR))
    b = min(x.size, live[-1] + int(tail * SR))
    y = x[a:b].copy()
    f = int(SR * 0.006)
    y[:f] *= np.linspace(0, 1, f)
    y[-f:] *= np.linspace(1, 0, f)
    return y


def read_audio(path: Path) -> np.ndarray:
    """Any audio file -> mono float32 at SR, through FFmpeg."""
    tmp = VIDEO / "build" / "tmp-read.wav"
    run([FFMPEG, "-y", "-v", "error", "-i", str(path), "-ac", "1", "-ar", str(SR), "-c:a", "pcm_f32le", str(tmp)])
    import soundfile as sf
    x, _ = sf.read(str(tmp), dtype="float32")
    tmp.unlink(missing_ok=True)
    return x


class Speaker:
    def __init__(self):
        from kokoro_onnx import Kokoro
        self.k = Kokoro(str(MODEL), str(VOICES))
        self.sig = f"{MODEL.stat().st_size}"

    def say_raw(self, text: str, voice: str, speed: float) -> np.ndarray:
        """Untrimmed speech, cached."""
        cache = VIDEO / "build" / "voice" / "cache"
        cache.mkdir(parents=True, exist_ok=True)
        key = hashlib.sha1(f"raw|{self.sig}|{voice}|{speed:.3f}|{text}".encode()).hexdigest()[:20]
        f = cache / f"{key}.npy"
        if f.exists():
            return np.load(f)
        samples, sr = self.k.create(text, voice=voice, speed=speed, lang="en-us")
        x = np.asarray(samples, dtype=np.float32)
        np.save(f, x)
        return x

    def say(self, text: str, voice: str, speed: float) -> np.ndarray:
        cache = VIDEO / "build" / "voice" / "cache"
        cache.mkdir(parents=True, exist_ok=True)
        key = hashlib.sha1(f"{self.sig}|{voice}|{speed:.3f}|{text}".encode()).hexdigest()[:20]
        f = cache / f"{key}.npy"
        if f.exists():
            return np.load(f)
        samples, sr = self.k.create(text, voice=voice, speed=speed, lang="en-us")
        assert sr == SR, sr
        x = trim(np.asarray(samples, dtype=np.float32))
        np.save(f, x)
        return x


def split_on_gaps(x: np.ndarray, n: int):
    """Cut one utterance into n pieces at its n-1 longest pauses. None when it has fewer pauses than that."""
    if n == 1:
        return [trim(x)]
    win = int(SR * 0.005)
    env = np.convolve(np.abs(x), np.ones(win) / win, mode="same")
    quiet = env < 10 ** (-48 / 20)
    runs, i = [], 0
    while i < len(x):
        if quiet[i]:
            j = i
            while j < len(x) and quiet[j]:
                j += 1
            if i > 0 and j < len(x) and (j - i) >= 0.07 * SR:
                runs.append((j - i, (i + j) // 2))
            i = j
        else:
            i += 1
    if len(runs) < n - 1:
        return None
    cuts = sorted(c for _, c in sorted(runs, reverse=True)[: n - 1])
    edges = [0] + cuts + [len(x)]
    return [trim(x[a:b]) for a, b in zip(edges[:-1], edges[1:])]


# ---------------------------------------------------------------- timeline

def synth_scene_clips(film, script, voice, speed, speaker):
    """clips[scene id] = list of arrays; recorded files, when present, give one array per scene instead."""
    recorded_dir = VIDEO / "build" / "voice" / film / "recorded"
    clips = {}
    for scene in script["scenes"]:
        rec = next((p for p in recorded_dir.glob(scene["id"] + ".*")), None) if recorded_dir.exists() else None
        if rec:
            clips[scene["id"]] = ("recorded", read_audio(rec))
        else:
            pieces = None
            if scene.get("bundle") and len(scene["cues"]) > 1:
                # The speech model is weak on very short utterances, so the cues of a scene are read as one
                # utterance and cut apart at its pauses.
                whole = speaker.say_raw(" ".join(c["spoken"] for c in scene["cues"]), voice, speed)
                pieces = split_on_gaps(whole, len(scene["cues"]))
            clips[scene["id"]] = ("synth", pieces or [speaker.say(c["spoken"], voice, speed) for c in scene["cues"]])
    return clips


def scene_layout(scene, kind, clips):
    """Cue offsets inside a scene (seconds from the scene start) and the scene's spoken length."""
    pre, gap = scene.get("pre", 0.1), scene.get("gap", 0.25)
    offs = []
    if kind == "recorded":
        total = clips.size / SR
        weights = [len(c["caption"]) for c in scene["cues"]]
        acc = 0.0
        for c, w in zip(scene["cues"], weights):
            d = total * w / sum(weights)
            offs.append((pre + acc, pre + acc + d))
            acc += d
        return offs, pre + total
    cursor = pre
    for i, (cue, x) in enumerate(zip(scene["cues"], clips)):
        cursor += cue.get("gap", gap) if i else 0
        d = x.size / SR
        offs.append((cursor, cursor + d))
        cursor += d
    return offs, cursor


def build_timeline(script, per_voice, shared_scene_len=None):
    """per_voice[voice] = {scene id: (kind, clips)}. Scene lengths are shared (the longest voice wins) so both
    voices fit the same picture. Returns {voice: timeline}."""
    fps, lead = script["fps"], script["lead"]
    layouts = {v: {s["id"]: scene_layout(s, *per_voice[v][s["id"]]) for s in script["scenes"]} for v in per_voice}
    scenes_len = {}
    for s in script["scenes"]:
        scenes_len[s["id"]] = max(layouts[v][s["id"]][1] for v in per_voice) + s.get("hold", 0.4)
        print("  scene %-9s " % s["id"] + "  ".join("%s %.2f s" % (v, layouts[v][s["id"]][1]) for v in per_voice))
    starts, cursor = {}, lead
    for s in script["scenes"]:
        starts[s["id"]] = cursor
        cursor += scenes_len[s["id"]]
    duration = cursor + script["tail"]
    # whole frames at 30, 60 and 120 fps, so a render has an exact length at every rate: a multiple of 1/30 s
    duration = round(duration * 30) / 30
    out = {}
    for v in per_voice:
        scenes = []
        for s in script["scenes"]:
            offs, _ = layouts[v][s["id"]]
            cues = [{"id": f"{s['id']}.{i + 1}", "start": round(starts[s["id"]] + a, 3), "end": round(starts[s["id"]] + b, 3),
                     "text": c["caption"], "spoken": c["spoken"]} for i, (c, (a, b)) in enumerate(zip(s["cues"], offs))]
            scenes.append({"id": s["id"], "start": round(starts[s["id"]], 3), "dur": round(scenes_len[s["id"]], 3), "cues": cues})
        out[v] = {"film": script["film"], "voice": v, "fps": fps, "duration": round(duration, 3), "lead": lead, "scenes": scenes, "title": script.get("title", "")}
        spec = script.get("poster")
        if spec:
            at = next(c["start"] for sc_ in scenes for c in sc_["cues"] if c["id"] == spec["cue"])
            out[v]["poster"] = round(min(duration - 0.5, at + spec.get("offset", 0.0)), 3)
    return out


def assemble(script, tl, clips_by_scene, total):
    """The narration track at SR: every clip placed at its cue time."""
    n = int(round(total * SR)) + SR
    track = np.zeros(n, dtype=np.float32)
    for scene in tl["scenes"]:
        kind, clips = clips_by_scene[scene["id"]]
        if kind == "recorded":
            first = next(s for s in script["scenes"] if s["id"] == scene["id"]).get("pre", 0.1)
            a = int(round((scene["start"] + first) * SR))
            track[a:a + clips.size] += clips[: n - a]
            continue
        for cue, x in zip(scene["cues"], clips):
            a = int(round(cue["start"] * SR))
            track[a:a + x.size] += x[: n - a]
    return track


# ---------------------------------------------------------------- captions and mastering

def vtt_time(t: float) -> str:
    h, rem = divmod(t, 3600)
    m, s = divmod(rem, 60)
    return f"{int(h):02d}:{int(m):02d}:{s:06.3f}"


def caption_cues(scene, min_len=1.1, max_chars=84):
    """The caption cues of one scene. A cue under min_len seconds is joined to its neighbour in the scene, so a
    caption never flashes by; the picture still cuts on every spoken cue."""
    cues = [dict(c) for c in scene["cues"]]
    i = 0
    while i < len(cues):
        c = cues[i]
        if c["end"] - c["start"] < min_len and len(cues) > 1:
            lo = i if i + 1 < len(cues) else i - 1
            a, b = cues[lo], cues[lo + 1]
            text = a["text"] + " " + b["text"]
            if len(text) <= max_chars:
                cues[lo:lo + 2] = [{"start": a["start"], "end": b["end"], "text": text}]
                i = max(0, lo - 1)
                continue
        i += 1
    # a caption that would be read faster than 19 characters a second stays up a little longer, as far as the next
    # caption (or 0.35 s past the end of the scene's last one) allows: the audio and the picture are not touched
    scene_end = scene["start"] + scene["dur"] - 0.01
    for k, c in enumerate(cues):
        need = len(c["text"]) / 19.0
        if c["end"] - c["start"] < need:
            limit = min(cues[k + 1]["start"] - 0.04 if k + 1 < len(cues) else c["end"] + 0.35, scene_end)
            c["end"] = max(c["end"], min(c["start"] + need, limit))
        if len(c["text"]) / max(c["end"] - c["start"], 0.01) > 20.5:
            floor = (cues[k - 1]["end"] + 0.04) if k else scene["start"]
            c["start"] = max(floor, c["start"] - min(0.12, len(c["text"]) / 20.5 - (c["end"] - c["start"]) + 0.01))
    return cues


KEEP = "\ue000"      # stands for a space inside a name that a caption must not break over two lines


def unbreakable(text: str, names: list) -> str:
    """A model name (and its effort) is one unit: the site's guard refuses 'Claude' or 'Haiku' alone, so the wrap never splits it."""
    for name in sorted(names, key=len, reverse=True):
        text = text.replace(name, name.replace(" ", KEEP))
    return text


def model_names() -> list:
    """Every spelling of a model the captions may hold, from build/facts.json: 'Claude Haiku 5.5, low effort', 'Claude Haiku 5.5', 'low effort'."""
    try:
        facts = json.loads((VIDEO / "build" / "facts.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return []
    out = []
    for m in (facts.get("models") or {}).values():
        out += [m["label"], m["name"]]
    return out


def write_vtt(tl, path: Path):
    lines = ["WEBVTT", ""]
    n = 1
    names = model_names()
    for scene in tl["scenes"]:
        for cue in caption_cues(scene):
            pieces = split_caption(unbreakable(cue["text"], names))
            total_w = sum(w for _, w in pieces)
            a = cue["start"]
            span = cue["end"] - cue["start"]
            for lines_, w in pieces:
                b = a + span * w / total_w
                lines += [str(n), f"{vtt_time(a)} --> {vtt_time(b)}", *[x.replace(KEEP, " ") for x in lines_], ""]
                n += 1
                a = b
    path.write_text("\n".join(lines), encoding="utf-8")


def write_wav(path: Path, x: np.ndarray, sr: int, channels=1):
    y = np.clip(x, -1, 1)
    pcm = (y * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm.tobytes())


def resample_to_48k(x24: np.ndarray, tmp: Path) -> np.ndarray:
    import soundfile as sf
    a = tmp.with_suffix(".24k.wav")
    b = tmp.with_suffix(".48k.wav")
    sf.write(str(a), x24, SR, subtype="FLOAT")
    run([FFMPEG, "-y", "-v", "error", "-i", str(a), "-ar", str(OUT_SR), "-c:a", "pcm_f32le", str(b)])
    y, _ = sf.read(str(b), dtype="float32")
    a.unlink(missing_ok=True)
    b.unlink(missing_ok=True)
    return y


def last_json(text: str) -> dict:
    """The JSON block FFmpeg's loudnorm prints at the end of its log."""
    start = text.rindex("{")
    return json.loads(text[start:text.index("}", start) + 1])


def loudnorm_two_pass(src: Path, dst: Path, target=-16.0, tp=-1.5, lra=9.0):
    """FFmpeg loudnorm, measured first and then applied linearly. Returns the measured values of the result."""
    base = f"loudnorm=I={target}:TP={tp}:LRA={lra}"
    p = run([FFMPEG, "-hide_banner", "-nostats", "-i", str(src), "-af", base + ":print_format=json", "-f", "null", "-"])
    m = last_json(p.stderr)
    flt = (f"{base}:measured_I={m['input_i']}:measured_TP={m['input_tp']}:measured_LRA={m['input_lra']}"
           f":measured_thresh={m['input_thresh']}:offset={m['target_offset']}:linear=true:print_format=json")
    p2 = run([FFMPEG, "-y", "-hide_banner", "-nostats", "-i", str(src), "-af", flt, "-ar", str(OUT_SR), "-c:a", "pcm_s16le", str(dst)])
    r = last_json(p2.stderr)
    return {"input_i": float(m["input_i"]), "input_tp": float(m["input_tp"]), "output_i": float(r["output_i"]), "output_tp": float(r["output_tp"])}


def lufs_of(path: Path) -> float:
    p = run([FFMPEG, "-hide_banner", "-nostats", "-i", str(path), "-af", "loudnorm=I=-16:print_format=json", "-f", "null", "-"])
    return float(last_json(p.stderr)["input_i"])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("film")
    ap.add_argument("--primary", default="af_heart", help="the voice that is muxed into the film")
    ap.add_argument("--voices", default="af_heart,am_michael")
    ap.add_argument("--speed", type=float, help="override the speed in the script")
    ap.add_argument("--no-music", action="store_true")
    args = ap.parse_args()

    facts = json.loads((VIDEO / "build" / "facts.json").read_text(encoding="utf-8"))
    script = load_script(args.film, facts)
    speed = args.speed or script["speed"]
    voices = [v for v in args.voices.split(",") if v]
    if args.primary not in voices:
        voices.insert(0, args.primary)
    speaker = Speaker()
    speeds = {v: script.get("voice_speed", {}).get(v, speed) for v in voices}
    per_voice = {v: synth_scene_clips(args.film, script, v, speeds[v], speaker) for v in voices}
    timelines = build_timeline(script, per_voice)

    outdir = VIDEO / "build"
    renders = VIDEO / "renders"
    renders.mkdir(exist_ok=True)
    report = {"film": args.film, "speed": speeds, "voices": {}}
    for v in voices:
        tl = timelines[v]
        narr24 = assemble(script, tl, per_voice[v], tl["duration"])
        narr = resample_to_48k(narr24, outdir / f"{args.film}.{v}.tmp")
        total = int(round(tl["duration"] * OUT_SR))
        narr = np.pad(narr, (0, max(0, total - narr.size)))[:total]
        stem = outdir / f"{args.film}.{v}.voice.wav"
        write_wav(stem, narr, OUT_SR)
        mix = np.stack([narr, narr], axis=1)
        music_info = None
        if args.film == "trailer" and not args.no_music:
            import music
            bed = music.make_bed(tl, OUT_SR, narr)
            bed_path = outdir / f"{args.film}.{v}.bed.wav"
            write_wav(bed_path, bed.reshape(-1), OUT_SR, channels=2)
            voice_lufs, bed_lufs = lufs_of(stem), lufs_of(bed_path)
            bed = bed * 10 ** ((voice_lufs - bed_lufs - BED_BELOW_DB) / 20)     # the bed sits exactly BED_BELOW_DB under the voice
            write_wav(bed_path, bed.reshape(-1), OUT_SR, channels=2)
            music_info = {"voice_lufs": voice_lufs, "bed_lufs": lufs_of(bed_path)}
            music_info["bed_below_voice_db"] = round(music_info["voice_lufs"] - music_info["bed_lufs"], 1)
            mix = mix + bed
        pre = outdir / f"{args.film}.{v}.premaster.wav"
        write_wav(pre, mix.reshape(-1), OUT_SR, channels=2)
        master = outdir / f"{args.film}.{v}.master.wav"
        meas = loudnorm_two_pass(pre, master)
        pre.unlink(missing_ok=True)
        write_vtt(tl, outdir / f"{args.film}.{v}.en.vtt")
        (outdir / f"{args.film}.{v}.timeline.json").write_text(json.dumps(tl, indent=1), encoding="utf-8")
        words = sum(len(c["spoken"].split()) for s in script["scenes"] for c in s["cues"])
        speech = sum((c["end"] - c["start"]) for s in tl["scenes"] for c in s["cues"])
        report["voices"][v] = {"duration": tl["duration"], "words": words, "speech_seconds": round(speech, 2),
                               "wpm": round(words / speech * 60), "loudness": meas, "music": music_info}
        print(f"{args.film} {v}: {tl['duration']:.2f} s, {words} words, {report['voices'][v]['wpm']} wpm, "
              f"master {meas['output_i']:.1f} LUFS, TP {meas['output_tp']:.1f} dB" + (f", bed {music_info['bed_below_voice_db']} dB under voice" if music_info else ""))
    # the timeline the pictures are made from: the primary voice
    tl = timelines[args.primary]
    (outdir / f"{args.film}.timeline.json").write_text(json.dumps(tl, indent=1), encoding="utf-8")
    (outdir / f"{args.film}.timeline.js").write_text("// Generated by tools/voice.py. Do not edit.\nwindow.TIMELINE = " + json.dumps(tl) + ";\n", encoding="utf-8")
    report["primary"] = args.primary
    (outdir / f"{args.film}.voice-report.json").write_text(json.dumps(report, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
