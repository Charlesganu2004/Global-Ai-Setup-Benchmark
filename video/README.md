# PeterGanuRusty films

Private source for the two films on the site: an explainer (about 76 seconds) and an ad (about 21 seconds). Both
are drawn in a browser page from the site's own numbers, spoken by a voice that runs on this machine, and
encoded here. Nothing is sent anywhere: no account, no hosted service, no plugin. This folder is private; only
what the build writes into `docs/media/` and `benchmark/data/video.json` is public.

## Render a film again

From this folder (`video\`), one command each:

    node tools/build.mjs explainer
    node tools/build.mjs ad

(`npm run explainer` and `npm run ad` do the same.) Add `--scale 2` for the 2160p version: the page is the same,
drawn at twice the pixels (`npm run explainer:2160`, `npm run ad:2160`). When `docs/data/showcase.js` changes
(new measurements, a new best configuration, `engine-v2` rows) run the same command: nothing is typed in by hand.

What a build does, in order:

1. `tools/facts.mjs` reads `docs/data/showcase.js` and picks the headline pair exactly as the site does
   (`App.headline` in `docs/assets/app.js`: the configuration flagged best and the first control, on the light
   class and the first question set). It also works out the four comparisons behind the range the site states (two
   model classes by two question sets) and the highlighted configuration's own run count. It writes
   `build/facts.json` (every number as digits and as spoken words) and `build/facts.js` (what the pages read). It
   stops with a message if the story in the script no longer holds, for example if the best configuration stops
   needing fewer tokens than no index in any of the four comparisons, or stops being fully correct in every run.
2. `tools/voice.py` fills the narration in `script/<film>.json` with those numbers, speaks each cue, measures
   every clip, builds the timeline (scene starts, cue starts, scene lengths) from the measurements, writes the
   captions, and masters the audio to about -16 LUFS with a true peak under -1.5 dB (FFmpeg `loudnorm`, two
   passes). The ad also gets its music bed here (`tools/music.py`).
3. `tools/render.mjs` opens `scenes/<film>.html` in several headless Edge windows (own profile folders under
   `video\`, each browser picks its own debugging port), asks the page to draw each frame with
   `window.PGR.seek(t)`, captures the frames over the DevTools protocol (PNG at 1080p; JPEG at quality 97 at 2160p,
   where a PNG takes five times as long) and pipes them in order into FFmpeg, which writes a near-lossless master
   (`build/<film>-<height>p.master.mp4`, H.264 at CRF 8).
4. `tools/build.mjs` encodes the final file in two passes to a size ceiling, muxes the voice, extracts the
   poster from a real frame, copies the captions, rewrites `benchmark/data/video.json`, makes a contact sheet and
   runs `tools/probe.py` (container facts, loudness, fast start, and the caption checks).

Everything lands in `docs/media/`: `pgr-<film>-1080p.mp4`, `pgr-<film>-2160p.mp4`, `pgr-<film>-poster.jpg`,
`pgr-<film>.en.vtt`. The other voice, the master videos and the muxed audio are kept in `renders/`.

Other commands: `node tools/shot.mjs <film> 3.2 12.5` writes single frames to `scratch/shots` (to look at them),
`node tools/check-numbers.mjs <film>` reads the text on screen at the end of every cue and fails on a figure that
is not in `showcase.js`, `node tools/montage.mjs out.png a.png b.png` tiles frames, `node tools/check-fresh.mjs`
fails when a file in `docs/media/` is older than `showcase.js` and was drawn from other figures or labels than
`showcase.js` holds today (each build keeps the figures it used in `renders/pgr-<film>-<height>p.facts.json`, because
the site's build rewrites `showcase.js` often without changing a figure).

Rules the pictures keep (the films are watched about 360 px wide, and the site's player puts captions and controls
over the lower third): every line that qualifies a claim is set in the `.q` size of `scenes/stage.css` (60 px or
more on the 1920 x 1080 stage, against a floor of 43 px in the ad and 32 px in the explainer, which is 4 and 3
percent of the frame height) and sits in the upper two thirds of the frame. Each figure is labelled as the comparison
it is (light class, first question set). The films set the product's fully-correct count nowhere against the no index
control's: they compare with it on tokens and turns only. The poster of each film is a frame whose main text is large,
in the upper two thirds, and clear of the circle the player's play button covers in the middle of the frame (about
180 px around the centre of the stage) and of its control bar (below y 790); the poster frame is `poster` in
`script/<film>.json`.

Voice only, after editing a line: `.venv\Scripts\python.exe -I tools\voice.py explainer`, then
`node tools/build.mjs explainer --skip-voice`. Only the changed cue is spoken again (clips are cached by text,
voice and speed in `build/voice/cache`).

## The voice

* Engine: Kokoro (82 million parameters) through `kokoro-onnx` and onnxruntime, on the CPU. The voices are
  synthetic and the films say nothing to hide that.
* Voices: `af_heart` (female) is muxed by default. `am_michael` (male) is rendered too, in the same scene
  lengths, and kept as audio in `renders/pgr-<film>-am_michael.m4a`. To mux the other one:
  `node tools/build.mjs <film> --primary am_michael`.
* Pace: `speed` in `script/<film>.json` (explainer 0.9, about 150 words a minute; ad 1.12, tighter), with a
  per-voice `voice_speed`. Scene lengths come from the audio, never from hand-set durations.
* Numbers are spoken as words filled from the same values that are drawn: `{tok_best}`, `{pct}`, `{agents}` and so
  on (the list is the `say` table in `build/facts.json`). Captions get the digit form of the same values.
* Pronunciation: the name is read correctly by the phonemizer as written. If a word is read wrongly, give the
  cue a `"say"` text next to its `"text"` (spoken and caption forms may differ).
* Short cues: scenes marked `"bundle": true` are read as one utterance and cut apart at the pauses, because the
  model is weak on very short utterances.
* Music (ad only): `tools/music.py` makes a low drone, a thump on every hard cut, a riser into the collapse and two
  pings from numpy alone. Hits are placed at the cue times that cut the picture. The bed is ducked under the voice
  and set 18 dB below it (integrated loudness); the brief asks for at least 14.

### Use a recorded voice instead

Record one audio file per scene and save them as `build/voice/<film>/recorded/<scene id>.wav` (`.mp3`, `.m4a` and
`.flac` work too). Scene ids are in `script/<film>.json` (`cold`, `change`, `gives`, `study`, `close` for the
explainer; `hook`, `mass`, `collapse`, `number`, `claim`, `close` for the ad). Then run
`node tools/build.mjs <film>`. A scene with a recorded file takes its length from that file; the cues inside it
are spread over the file by their number of characters, so the reveals are close but not exact. Leave a scene out
and it keeps the synthetic voice.

## The picture

* `scenes/lib.js`: easing (including the site's spring), seeded random numbers, canvas helpers, text helpers,
  the mark. `scenes/graphics.js`: the four things PGR gives an agent, drawn as outcomes. `scenes/explainer.js`
  and `scenes/ad.js`: the films. `scenes/stage.css`: the site's tokens (colours, system fonts, grain).
* Every frame is a function of time: no animation runs on its own clock, nothing accumulates between frames, so
  frames can be drawn in any order by any number of windows and a re-render gives the same file.
* Motion carries meaning. One particle is 1k tokens of the no index total (365). The read head that counts them,
  the five turns, the lane that shortens to 130 particles and the numerals that count down are all driven by
  the numbers. The stream's pulse runs at the speed of its turns. Reveals are keyed to the start of the cue that
  names them (`cueAt` finds a phrase inside a cue from its place in the spoken text).
* Fonts are the site's system fonts (`Segoe UI Variable Display` on this machine). A machine without them falls
  back to the next family in the stack, and the type then differs.
* What may be shown: outcomes, the study in general terms, the measured numbers, the limits, private preview.
  Never a command, a flag, a prompt, a file path of the product, a method, another tool's name, a vendor or a model.
  Check the public files with `python -I benchmark\harness\build_site.py --guard docs` from the repository root.

## Installed, and where

Nothing is global. All of it sits in this folder and is ignored by git.

| What | Where | Version | Licence | Download |
| --- | --- | --- | --- | --- |
| Python virtual environment | `.venv` | Python 3.11.5 (from an existing Anaconda install), pip 26.2.1 | PSF | none |
| kokoro-onnx | `.venv` | 0.6.1 | MIT (project repository; the wheel carries no licence field) | 26 kB |
| onnxruntime | `.venv` | 1.31.0 | MIT | 14.9 MB |
| numpy | `.venv` | 2.4.6 | BSD-3-Clause | 12.6 MB |
| soundfile | `.venv` | 0.14.0 | BSD-3-Clause (libsndfile inside is LGPL-2.1) | 1.0 MB |
| av (PyAV) | `.venv` | 18.1.0 | BSD-3-Clause (bundles LGPL FFmpeg libraries) | 27.6 MB |
| imageio-ffmpeg | `.venv` | 0.6.0 | BSD-2-Clause | 31.2 MB |
| espeakng-loader | `.venv` | 0.2.4 | MIT (it carries eSpeak NG, GPL-3.0) | 9.4 MB |
| phonemizer | `.venv` | 3.4.0 | GPL-3.0-or-later | 108 kB |
| their dependencies | `.venv` | cffi 2.1.1, pycparser 3.11, protobuf 7.36.2, flatbuffers 25.12.19, packaging 26.3, joblib 1.6.0, cloudpickle 3.1.2, attrs 26.1.0, dlinfo 2.0.0, typing-extensions 4.16.0 | permissive | under 2 MB together |
| FFmpeg | `bin/ffmpeg.exe` (the binary inside imageio-ffmpeg, copied) | 7.1 essentials build (gyan.dev) | GPL-3.0 (it includes libx264); run as a separate program, never shipped | 87.6 MB |
| Kokoro model | `models/kokoro-v1.0.onnx` | v1.0 | Apache-2.0 | 325,532,387 bytes |
| Kokoro voices | `models/voices-v1.0.bin` | v1.0 | Apache-2.0 | 28,214,398 bytes |
| Browser | not installed here: Microsoft Edge from the system | 154.0.4258.48 | system | none |
| Node.js | not installed here: the system's | 24.15.0 | MIT | none |

The two model files come from the `model-files-v1.0` release of the kokoro-onnx repository. No npm package is
used: the DevTools client is Node's built-in WebSocket.

Set up again on a clean machine (PowerShell, from `video\`):

    python -m venv .venv
    .venv\Scripts\python.exe -m pip install -r requirements.txt
    mkdir bin, models
    copy .venv\Lib\site-packages\imageio_ffmpeg\binaries\ffmpeg-win-x86_64-v7.1.exe bin\ffmpeg.exe
    Invoke-WebRequest https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx -OutFile models\kokoro-v1.0.onnx
    Invoke-WebRequest https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin -OutFile models\voices-v1.0.bin

A browser other than Edge: set `PGR_BROWSER` to its executable. Another FFmpeg: `PGR_FFMPEG`. Another Python:
`PGR_PYTHON`.

## Layout

    script/      narration per scene, one JSON file per film
    scenes/      the films as HTML, CSS and JavaScript on a 1920 x 1080 stage
    tools/       facts, voice, music, render, build, probe, checks
    build/       generated: facts, timelines, audio, masters (ignored)
    renders/     generated: the other voice, masters, muxed audio (ignored)
    models/ bin/ .venv/   installed tools (ignored)

## Limits

* The voice is synthetic and was checked by numbers (loudness, length, speech against captions) and by the phonemes the model is given, never by ear.
* The explainer uses the headline pair of the site. A faster `engine-v2` row appears in the leaderboard when it is
  in `showcase.js`; an `engine` speed table is not used, because its shape is not known yet.
* The range and the run count come from the `configs` points of `showcase.js` (the best configuration against no
  index, for each model class and question set both were run on); the word on the highlighted row is
  `data.labels.best` or `data.best_label` when `showcase.js` has one, else "Most reliable". Both films stop early
  if the highlighted configuration is no longer fully correct in every run on both question sets, or if the range
  differs from the one in `data.versus` (the site's own statement of it).
* The captions are the browser's own and the site's player lifts them to the fourth line from the bottom, so on a
  1080 frame they cover roughly the band from y 700 to y 830 (an estimate from the default cue size, not a
  measurement: a headless browser does not paint video). The heaviest leaderboard rows of the explainer lie in that
  band for about five seconds while a caption says what the rows are; everything that qualifies a claim stays
  above it.

