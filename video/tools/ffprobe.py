"""ffprobe for the films, with PyAV (this folder ships FFmpeg without ffprobe). Prints one JSON object per file:
codec, profile, level, size, frame rate, frame count against duration x fps, bit rate, colour tags, the codec string a
browser needs in the type attribute (avc1.... or hvc1....), and the audio stream.

    .venv\\Scripts\\python.exe -I tools\\ffprobe.py file.mp4 [more.mp4 ...]
"""
import json
import sys
from fractions import Fraction
from pathlib import Path

import av


def hevc_string(ex: bytes) -> str:
    # hvcC: [1] profile space/tier/idc, [2:6] compatibility flags, [6:12] constraint flags, [12] level
    b1 = ex[1]
    space = "" if (b1 >> 6) == 0 else "ABC"[(b1 >> 6) - 1]
    tier = "H" if (b1 >> 5) & 1 else "L"
    idc = b1 & 31
    flags = int.from_bytes(ex[2:6], "big")
    rev = int("{:032b}".format(flags)[::-1], 2)
    cons = list(ex[6:12])
    while cons and cons[-1] == 0:
        cons.pop()
    cstr = ".".join("%X" % c for c in cons) if cons else "0"
    return "hvc1.%s%d.%X.%s%d.%s" % (space, idc, rev, tier, ex[12], cstr)


def probe(path: Path) -> dict:
    out = {"file": path.name, "bytes": path.stat().st_size}
    with av.open(str(path)) as c:
        v = next(s for s in c.streams if s.type == "video")
        cc = v.codec_context
        ex = bytes(cc.extradata or b"")
        name = cc.name
        if name == "h264" and len(ex) > 4 and ex[0] == 1:
            cs = "avc1.%02x%02x%02x" % (ex[1], ex[2], ex[3])
        elif name == "hevc" and len(ex) > 13 and ex[0] == 1:
            cs = hevc_string(ex)
        else:
            cs = None
        frames = 0
        first = last = None
        vbytes = 0
        for p in c.demux(v):
            if p.size:
                frames += 1
                vbytes += p.size
        dur = float(c.duration / av.time_base) if c.duration else 0.0
        fps = float(v.average_rate) if v.average_rate else 0.0
        out.update({
            "codec": name, "profile": cc.profile, "level": cc.level, "pix_fmt": cc.pix_fmt, "width": cc.width, "height": cc.height,
            "fps": fps, "r_frame_rate": str(v.base_rate), "frames": frames, "duration": round(dur, 3), "expected_frames": round(dur * fps),
            "frame_count_matches": abs(frames - dur * fps) <= 1.5, "video_bit_rate_kbps": round(vbytes * 8 / max(dur, 1e-6) / 1000),
            "overall_bit_rate_kbps": round(out["bytes"] * 8 / max(dur, 1e-6) / 1000), "megabytes": round(out["bytes"] / 1e6, 2),
            "mbps": round(out["bytes"] * 8 / max(dur, 1e-6) / 1e6, 2), "codec_string": cs,
            "type": ('video/mp4; codecs="%s"' % cs) if cs else None,
            "colorspace": str(getattr(cc, "colorspace", "")), "color_range": str(getattr(cc, "color_range", "")),
            "color_primaries": str(getattr(cc, "color_primaries", "")), "color_trc": str(getattr(cc, "color_trc", "")),
        })
        a = next((s for s in c.streams if s.type == "audio"), None)
        if a:
            out["audio"] = {"codec": a.codec_context.name, "channels": a.codec_context.channels, "sample_rate": a.codec_context.sample_rate}
        # fast start: the moov atom comes before mdat
        head = path.read_bytes()[:4096] if path.stat().st_size else b""
        out["faststart"] = head.find(b"moov") != -1 and (head.find(b"mdat") == -1 or head.find(b"moov") < head.find(b"mdat"))
    return out


if __name__ == "__main__":
    for f in sys.argv[1:]:
        print(json.dumps(probe(Path(f)), indent=1))
