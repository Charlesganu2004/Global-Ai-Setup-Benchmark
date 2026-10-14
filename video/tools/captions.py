"""Writes the captions of a film again from its timeline (build/<film>.timeline.json), without speaking anything:

    .venv\\Scripts\\python.exe -I tools\\captions.py trailer|explainer

voice.py writes them as part of a voice run; this is for when only the way captions are cut changed. The captions are
written to build/<film>.<voice>.en.vtt, the file tools/build.mjs copies to docs/media.
"""
import importlib.util
import json
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("voice", here / "voice.py")
voice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(voice)

film = sys.argv[1]
tl = json.loads((voice.VIDEO / "build" / f"{film}.timeline.json").read_text(encoding="utf-8"))
out = voice.VIDEO / "build" / f"{film}.{tl['voice']}.en.vtt"
voice.write_vtt(tl, out)
print(out)
