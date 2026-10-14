# PeterGanuRusty films

Source for the two films on the site: an explainer (about 95 seconds) and a trailer (about 30 seconds). Both
are drawn in a browser page from the site's own numbers, spoken by a voice that runs on this machine, and
encoded here, at up to 3840 x 2160 and 120 frames a second. Nothing is sent anywhere: no account, no hosted service, no plugin. This folder is in the repository but is not served by the site; only
what the build writes into `docs/media/` and `benchmark/data/video.json` is on the site.

## Render a film again

From this folder (`video\`), one command each:

    node tools/build.mjs explainer              review render: 1920 x 1080, 60 fps, H.264
    node tools/build.mjs trailer
    node tools/build.mjs trailer --stretch      10 seconds at 3840 x 2160, 120 fps, HEVC (proof of the full path, into renders/)
    node tools/build.mjs trailer --finals       the final files: 2160p120 HEVC (drawn in chunks, so a stopped render resumes), then every other file from it
    node tools/build.mjs trailer --derive       the other files again from the 2160p120 file that is there (no frame is drawn)

(`npm run explainer`, `npm run trailer`, `npm run trailer:stretch`, `npm run trailer:finals` do the same.) The page is the
same at every size, drawn at the scale of the render. When `docs/data/showcase.js` changes (new measurements, a new best
configuration, `engine-v2` rows) run the same command: nothing is typed in by hand. `npm run check` runs every check.

What a build does, in order:

1. `tools/facts.mjs` reads `docs/data/showcase.js` and picks the headline pair exactly as the site does
   (`App.headline` in `docs/assets/app.js`: the configuration flagged best and the first control, on the smallest
   model and the first question set). The models are named as the runs used them: `benchmark/data/models.json` gives
   the name and the effort of each (Claude Haiku 5.5, low effort), and a name that differs from the one the site data
   carries stops the build. It also works out the four comparisons behind the range the site states (two
   models by two question sets) and the highlighted configuration's own run count. It writes
   `build/facts.json` (every number as digits and as spoken words) and `build/facts.js` (what the pages read). It
   stops with a message if the story in the script no longer holds, for example if the best configuration stops
   needing fewer tokens than no index in any of the four comparisons, or stops being fully correct in every run.
2. `tools/voice.py` fills the narration in `script/<film>.json` with those numbers, speaks each cue, measures
   every clip, builds the timeline (scene starts, cue starts, scene lengths) from the measurements, writes the
   captions, and masters the audio to about -16 LUFS with a true peak under -1.5 dB (FFmpeg `loudnorm`, two
   passes). The trailer also gets its music bed here (`tools/music.py`).
3. `tools/render.mjs` opens `scenes/<film>.html` in several headless Edge windows (own profile folders under
   `video\`, each browser picks its own debugging port, ANGLE on Direct3D 11 so that canvas and WebGL run on the graphics
   card), asks the page to draw each frame with `window.PGR.seek(t)`, captures the frames over the DevTools protocol
   (JPEG at quality 97 at 2160p and 98 at 1080p: a PNG takes three to seven times as long to come out of the browser) and
   pipes them in order into one FFmpeg. The final render draws 3840 x 2160 at 120 frames a second in chunks of 10 seconds
   (each chunk encoded at once by the graphics card, so that a render which stops starts again at the first chunk that is
   missing) from a copy of `scenes/` made at the start, so edits made while it runs do not reach it.
4. `tools/build.mjs` muxes the voice, extracts the poster from a real frame, copies the captions, rewrites
   `benchmark/data/video.json` (a list of sources per film, best first, each with its codec string, size and bit rate) and
   runs the site's own guard on the captions (`benchmark/harness/build_site.py --guard`) and `tools/probe.py` (container
   facts, loudness, fast start, and the caption checks). The best file is the 2160p120 HEVC of the graphics card (`hevc_nvenc`,
   constant quality 18, which is visually lossless on these graphics). Every other file is made from that file, so no frame is
   drawn twice and each is encoded with its size in mind: x264 High level 4.2 for 1080p60, x264 High level 5.2 for 2160p60,
   and x265 in two passes for the 2160p120 file under 95 MB. A 2160p file of 95 MB or more is not put in `docs/media/`
   (GitHub refuses 100 MB): it stays in `renders/`, is listed in `benchmark/data/video.json` as `release:<file name>`, and is attached to a release.

Everything that goes on the site lands in `docs/media/`: `pgr-<film>-1080p60.mp4`, `pgr-<film>-2160p60.mp4`,
`pgr-<film>-2160p120-small.mp4`, `pgr-<film>-poster.jpg`, `pgr-<film>-poster-2160.jpg`, `pgr-<film>.en.vtt`. The 2160p120 file
and the 2160p60 file when they are over 95 MB, the other voice and the stretch proofs are kept in `renders/` (not in git).

Two helpers for a finished film: `.venv/Scripts/python.exe -I tools/captions.py <film>` writes the captions again from the timeline (when
only the way captions are cut changed), and `node tools/patch-tail.mjs <film> <from-frame>` draws only the last seconds again and joins
them to the unchanged part of the best file (when only the end of a film changed); `node tools/build.mjs <film> --derive` then makes the
other files again.

Other commands: `node tools/check-finals.mjs <film> [--psnr]` probes every final file (codec, level, frame count against duration
x fps, bit rate, fast start, colour tags), fails on a file of 95 MB or more in `docs/media/`, measures how long the closing card is
held at full brightness (at least 3 seconds), looks for a doubled or dropped frame across fast motion and, with `--psnr`, says how close each
smaller file is to the best one; `node tools/check-rate.mjs <film>` renders one second at 30, 60 and 120 frames a second through
the real renderer and compares the decoded frames; `node tools/check-nograin.mjs` fails if any scene uses grain, noise or a dither, in the code or as measured in
the picture; `node tools/check-fps.mjs <film>` draws 12 timestamps the way a render at 30, 60 and 120 frames a second would
and compares every pixel (they match to within 2 levels of 255); `node tools/shot.mjs <film> 3.2 12.5` writes single frames to `scratch/shots` (to look at them),
`node tools/check-numbers.mjs <film>` reads the text on screen every half second and at every cue and fails on a figure that
is not in `showcase.js` (a numeral counting up towards a figure is let through only short of that figure), on a word that must
not be there (a model other than the three of `models.json`, a vendor, a class such as "light class", a tier letter, "private preview",
"ad"; in the script, the narration, the captions and the picture), and on display type past the margins or a string cut short;
`node tools/check-numbers.mjs selftest` plants each kind of mistake, and renders the film with a wrong headline figure, and says whether
the check catches it, `node tools/montage.mjs out.png a.png b.png` tiles frames, `node tools/check-fresh.mjs`
fails when a file in `docs/media/` is older than `showcase.js` and was drawn from other figures or labels than
`showcase.js` holds today (each build keeps the figures it used in `renders/pgr-<film>-<height>p.facts.json`, because
the site's build rewrites `showcase.js` often without changing a figure).

Rules the pictures keep (the films are watched about 360 px wide, and the site's player puts captions and controls
over the lower third): every line that qualifies a claim is set in the `.q` size of `scenes/stage.css` (60 px or
more on the 1920 x 1080 stage) and sits in the upper two thirds of the frame. Each figure is labelled as the comparison
it is (Claude Haiku 5.5, low effort, first question set). The films set the product's fully-correct count nowhere against the no index
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
* Pace: `speed` in `script/<film>.json` (explainer and trailer about 1.28, the trailer's `voice_speed` for `am_michael` 1.4), with a
  per-voice `voice_speed`. Scene lengths come from the audio, never from hand-set durations.
* Numbers are spoken as words filled from the same values that are drawn: `{tok_best}`, `{pct}`, `{agents}` and so
  on (the list is the `say` table in `build/facts.json`). Captions get the digit form of the same values.
* Pronunciation: the name is read correctly by the phonemizer as written. The first mention says the name and then the short
  form, "PeterGanuRusty, PGR for short", and the three letters are spoken as letters: the cue has `"say": "... P G R for short."`
  next to its `"text"` (spoken and caption forms may differ). Measured with Kokoro (af_heart): `PGR` 0.64 s with the letters run
  together, `P.G.R.` 0.89 s with a pause after each, `P G R` 0.71 s with three separately stressed letters, which is the one kept.
* Short cues: scenes marked `"bundle": true` are read as one utterance and cut apart at the pauses, because the
  model is weak on very short utterances.
* Music (trailer only): `tools/music.py` makes a low drone, a thump on every hard cut, a riser into the collapse and two
  pings from numpy alone. Hits are placed at the cue times that cut the picture. The bed is ducked under the voice
  and set 18 dB below it (integrated loudness); the brief asks for at least 14.

### Use a recorded voice instead

Record one audio file per scene and save them as `build/voice/<film>/recorded/<scene id>.wav` (`.mp3`, `.m4a` and
`.flac` work too). Scene ids are in `script/<film>.json` (`cold`, `change`, `gives`, `study`, `close` for the
explainer; `hook`, `mass`, `name`, `collapse`, `number`, `claim`, `close` for the trailer). Then run
`node tools/build.mjs <film>`. A scene with a recorded file takes its length from that file; the cues inside it
are spread over the file by their number of characters, so the reveals are close but not exact. Leave a scene out
and it keeps the synthetic voice.

## The picture

* `scenes/lib.js`: easing (including the site's spring), seeded random numbers, canvas helpers, text helpers, the mark, and the
  engine: each frame is drawn n times inside a fixed 1/120 s shutter (the scene is drawn first at the two ends of the shutter
  with a motion probe on: text, panels, glows and every dot of a cloud note where they are, and n is the largest distance any of
  them travelled, in output pixels, over 1.5, from 1 for a still frame to 28), the pictures are
  averaged in a float texture (real motion blur), a bloom chain adds light to the bright parts, and WebGL shows the result.
  `scenes/kit.js`: the camera, the floor, the bokeh, the token cloud, the race of the two agents. `scenes/graphics.js`: the four
  things PGR gives an agent, drawn as outcomes. `scenes/explainer.js` and `scenes/trailer.js`: the films. `scenes/stage.css`:
  the site's tokens (colours, system fonts). There is no grain, noise or dither anywhere.
* Every frame is a function of time: no animation runs on its own clock, nothing accumulates between frames, so
  frames can be drawn in any order by any number of windows and a re-render gives the same file.
* Motion carries meaning. One dot is 100 tokens: the no index total is 2,937 dots and PGR's is 1,340. The read head that
  counts them, the race of the two agents (both read at the same pace, so only the totals differ) and the numerals that count up
  are all driven by the numbers. Reveals are keyed to the start of the cue that
  names them (`cueAt` finds a phrase inside a cue from its place in the spoken text).
* Fonts are the site's system fonts (`Segoe UI Variable Display` on this machine). A machine without them falls
  back to the next family in the stack, and the type then differs.
* What may be shown: outcomes, the study in general terms, the measured numbers, the limits, and the install command of the
  public release (taken from `benchmark/data/install.json`). The terminals show an invented example session (`parse_config`,
  `config.py`; a small line says "Illustration: example names" while they are on screen). Never a flag of the product, a prompt, a method, another tool's name or a vendor. The product is
  public: no film says or shows a private preview. The models are the exception to "never a model": the films name the three
  models of `benchmark/data/models.json`, as the runs used them (Claude Haiku 5.5, low effort), and never as a class (light, mid, top) or a tier.

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
  index, for each model and question set both were run on); the word on the highlighted row is
  `data.labels.best` or `data.best_label` when `showcase.js` has one, else "Most reliable". Both films stop early
  if the highlighted configuration is no longer fully correct in every run on both question sets, or if the range
  differs from the one in `data.versus` (the site's own statement of it).
* The captions are the browser's own and the site's player lifts them to the fourth line from the bottom, so on a
  1080 frame they cover roughly the band from y 700 to y 830 (an estimate from the default cue size, not a
  measurement: a headless browser does not paint video). The heaviest leaderboard rows of the explainer lie in that
  band for about five seconds while a caption says what the rows are; everything that qualifies a claim stays
  above it.

