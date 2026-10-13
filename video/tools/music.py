"""A small original music bed for the ad, made from nothing but numpy.

It is not a song. It is a low drone, a thump on every hard cut, a riser into the collapse, a bright ping when the
filament lights and when the mark lands, and a soft chord under the name. Every hit is placed from the cue times
of the timeline, the same numbers that cut the picture, so sound and cut land together. The bed is ducked under the
voice and scaled by voice.py to sit 18 dB below it.
"""
import numpy as np


def _env_exp(n, sr, tau):
    return np.exp(-np.arange(n) / (tau * sr))


def _thump(sr, f0=150.0, f1=44.0, dur=0.34, tau=0.09):
    n = int(dur * sr)
    t = np.arange(n) / sr
    f = f1 + (f0 - f1) * np.exp(-t / 0.05)
    phase = 2 * np.pi * np.cumsum(f) / sr
    body = np.sin(phase) * _env_exp(n, sr, tau)
    click = np.random.default_rng(1).standard_normal(n) * _env_exp(n, sr, 0.004) * 0.22
    return (body + click).astype(np.float64)


def _ping(sr, f=1318.5, dur=1.6, tau=0.35):
    n = int(dur * sr)
    t = np.arange(n) / sr
    s = np.sin(2 * np.pi * f * t) + 0.45 * np.sin(2 * np.pi * f * 2.01 * t) * _env_exp(n, sr, tau * 0.4) + 0.3 * np.sin(2 * np.pi * f * 0.5 * t)
    return s * _env_exp(n, sr, tau) * np.minimum(1, t / 0.004)


def _bandpass_noise(n, sr, f_lo, f_hi, seed=3):
    """Noise whose centre frequency rises from f_lo to f_hi, a riser, by summing swept resonant bands."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) / sr
    f = f_lo * (f_hi / f_lo) ** (t / (n / sr))
    phase = 2 * np.pi * np.cumsum(f) / sr
    out = np.zeros(n)
    for k in range(6):
        out += np.sin(phase * (1 + 0.013 * k) + rng.uniform(0, 6.28)) * (0.6 + 0.4 * rng.random())
    noise = rng.standard_normal(n)
    # a one-pole high pass keeps the noise from rumbling
    hp = noise - np.convolve(noise, np.ones(40) / 40, mode="same")
    return out * 0.5 + hp * 0.35


def _place(bed, x, at, sr, gain=1.0, pan=0.0):
    a = int(round(at * sr))
    if a >= len(bed):
        return
    m = min(len(x), len(bed) - a)
    l = np.sqrt(0.5 * (1 - pan)) * 1.414
    r = np.sqrt(0.5 * (1 + pan)) * 1.414
    bed[a:a + m, 0] += x[:m] * gain * l
    bed[a:a + m, 1] += x[:m] * gain * r


def _duck_gain(voice, sr, depth_db=-12.0, hold=0.3, merge=0.8, attack=0.2, release=0.8):
    """Gain curve that dips under the voice: gaps shorter than `merge` seconds are bridged, as the audio guide asks."""
    rate = 200
    hop = sr // rate
    m = len(voice) // hop
    rms = np.sqrt((voice[:m * hop].reshape(m, hop).astype(np.float64) ** 2).mean(axis=1))
    active = (rms > 0.004).astype(np.float64)
    def dilate(a, sec):
        k = max(1, int(sec * rate))
        return (np.convolve(a, np.ones(2 * k + 1), mode="same") > 0).astype(np.float64)
    def erode(a, sec):
        k = max(1, int(sec * rate))
        return (np.convolve(a, np.ones(2 * k + 1), mode="same") >= 2 * k + 1 - 0.5).astype(np.float64)
    a = erode(dilate(active, merge / 2), merge / 2)
    a = np.maximum(a, np.concatenate([np.zeros(int(hold * rate)), a])[:len(a)])   # hold after the voice
    g = np.empty(m)
    cur = 0.0
    ka, kr = 1 - np.exp(-1 / (attack * rate)), 1 - np.exp(-1 / (release * rate))
    for i in range(m):
        target = a[i]
        cur += (target - cur) * (ka if target > cur else kr)
        g[i] = cur
    gain = 10 ** (depth_db / 20)
    curve = 1 - (1 - gain) * g
    return np.interp(np.arange(len(voice)) / sr, (np.arange(m) + 0.5) / rate, curve)


def make_bed(tl, sr, voice):
    """Returns a stereo float array (n, 2) the length of the voice, peak well under 1."""
    n = len(voice)
    t = np.arange(n) / sr
    bed = np.zeros((n, 2))
    sc = {s["id"]: s for s in tl["scenes"]}
    cue = lambda sid, k: sc[sid]["cues"][k - 1]["start"]
    dur = tl["duration"]
    # the second hook cue is "Again. And again.": its second half is where the picture cuts to "and again."
    hook2 = sc["hook"]["cues"][1]
    again = hook2["start"] + (hook2["end"] - hook2["start"]) * len("Again. ") / len(hook2["spoken"])

    # drone: A1 with its fifth and octave, a slow swell that opens at the collapse and settles for the close
    sw = np.interp(t, [0, cue("hook", 1), cue("mass", 1), cue("collapse", 1), cue("number", 1), cue("close", 1), dur],
                   [0.15, 0.45, 0.6, 0.35, 0.8, 0.55, 0.3])
    drone = (np.sin(2 * np.pi * 55.0 * t) + 0.55 * np.sin(2 * np.pi * 82.41 * t + 0.4 * np.sin(2 * np.pi * 0.11 * t)) + 0.35 * np.sin(2 * np.pi * 110.3 * t)) * sw
    bed[:, 0] += drone * 0.5
    bed[:, 1] += drone * 0.5

    # a thump on every hard cut: the picture cuts on the same instants
    cuts = [cue("hook", 1), cue("hook", 2), again, cue("mass", 1), cue("mass", 2), cue("collapse", 1), cue("collapse", 2),
            cue("number", 1), cue("claim", 1), cue("close", 1)]
    strength = [0.7, 0.85, 1.0, 0.9, 0.55, 1.0, 0.9, 1.0, 1.0, 0.8]
    th = _thump(sr)
    for c, g in zip(cuts, strength):
        _place(bed, th, c, sr, 1.0 * g)
    # quiet ticks between the thumps give the cuts a pulse
    tick = np.random.default_rng(5).standard_normal(int(0.02 * sr)) * _env_exp(int(0.02 * sr), sr, 0.004)
    tick = tick - np.convolve(tick, np.ones(8) / 8, mode="same")
    k = cue("hook", 1)
    while k < cue("close", 1) - 0.2:
        _place(bed, tick, k, sr, 0.10, pan=np.sin(k * 7.0) * 0.5)
        k += 0.25

    # the riser into the collapse, and a sub hit as it lands
    r0 = cue("collapse", 1) - 1.25
    nr = int(1.25 * sr)
    riser = _bandpass_noise(nr, sr, 220, 5200) * np.linspace(0, 1, nr) ** 2
    _place(bed, riser, r0, sr, 0.5)
    drop = int(0.07 * sr)
    bed[int((cue("collapse", 1)) * sr) - drop:int((cue("collapse", 1)) * sr), :] *= 0.15
    _place(bed, _thump(sr, 120, 36, 0.8, 0.22), cue("collapse", 1), sr, 1.6)

    # the filament lights, and the mark lands
    _place(bed, _ping(sr, 1318.5), cue("number", 1), sr, 0.45, pan=-0.2)
    _place(bed, _ping(sr, 880.0, 2.2, 0.55), cue("close", 1) + 0.35, sr, 0.5, pan=0.2)

    # a soft chord under the name
    c0 = cue("close", 1)
    nc = int((dur - c0) * sr)
    tc = np.arange(nc) / sr
    chord = sum(np.sin(2 * np.pi * f * tc + i) * a for i, (f, a) in enumerate([(110.0, 1.0), (164.81, 0.7), (220.0, 0.6), (329.63, 0.35)]))
    chord *= np.minimum(1, tc / 0.9) * np.minimum(1, (dur - c0 - tc) / 0.5)
    _place(bed, chord, c0, sr, 0.3)

    bed *= _duck_gain(voice, sr)[:, None]
    # a short fade at the very end and at the very start
    f = int(0.25 * sr)
    bed[-f:] *= np.linspace(1, 0, f)[:, None]
    peak = np.abs(bed).max()
    if peak > 0:
        bed *= 0.5 / peak
    return bed
