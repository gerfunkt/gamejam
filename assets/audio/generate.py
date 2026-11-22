#!/usr/bin/env python3
"""
Procedural foley SFX and radio-filtered pilot voice asset generator.

Usage:
  python3 web/assets/audio/generate.py --foley         # Generate procedural foley WAVs
  python3 web/assets/audio/generate.py --tts-raw       # Run macOS 'say' into /tmp/gamejam_tts
  python3 web/assets/audio/generate.py --voice-filter  # Apply VHF radio filter & write manifest
  python3 web/assets/audio/generate.py --all           # Run all steps
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import struct
import subprocess
import tempfile
import wave
from pathlib import Path
from typing import Callable, List, Tuple

SAMPLE_RATE = 22050
OUT_DIR = Path(__file__).resolve().parent
FOLEY_DIR = OUT_DIR / "foley"
VOICE_DIR = OUT_DIR / "voice"
RAW_TTS_DIR = Path(tempfile.gettempdir()) / "gamejam_tts"


def write_wav(path: Path, samples: List[float], sample_rate: int = SAMPLE_RATE) -> dict:
    """Write normalized float samples in [-1.0, 1.0] to a 16-bit mono PCM WAV file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    peak = max((abs(s) for s in samples), default=1.0)
    scale = 0.92 / peak if peak > 0.92 else 1.0
    pcm = bytearray(len(samples) * 2)
    for i, s in enumerate(samples):
        val = int(max(-1.0, min(1.0, s * scale)) * 32767.0)
        struct.pack_into("<h", pcm, i * 2, val)
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sample_rate)
        wf.writeframes(pcm)
    return {
        "file": str(path.relative_to(OUT_DIR)).replace("\\", "/"),
        "sample_rate": sample_rate,
        "frames": len(samples),
        "duration_s": round(len(samples) / sample_rate, 3),
        "bytes": path.stat().st_size,
    }


def read_wav_mono(path: Path) -> Tuple[List[float], int]:
    """Read a 16-bit PCM WAV file into float samples in [-1.0, 1.0]."""
    with wave.open(str(path), "rb") as wf:
        channels = wf.getnchannels()
        rate = wf.getframerate()
        nframes = wf.getnframes()
        raw = wf.readframes(nframes)
    total = nframes * channels
    ints = struct.unpack(f"<{total}h", raw)
    if channels > 1:
        ints = ints[::channels]
    return [x / 32768.0 for x in ints], rate


class Biquad:
    """Standard RBJ audio biquad filter."""

    def __init__(self, b0: float, b1: float, b2: float, a1: float, a2: float):
        self.b0 = b0
        self.b1 = b1
        self.b2 = b2
        self.a1 = a1
        self.a2 = a2
        self.x1 = 0.0
        self.x2 = 0.0
        self.y1 = 0.0
        self.y2 = 0.0

    def step(self, x: float) -> float:
        y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2
        self.x2 = self.x1
        self.x1 = x
        self.y2 = self.y1
        self.y1 = y
        return y

    def process(self, samples: List[float]) -> List[float]:
        return [self.step(s) for s in samples]

    @staticmethod
    def lowpass(freq: float, q: float = 0.707, sr: int = SAMPLE_RATE) -> "Biquad":
        w0 = 2.0 * math.pi * max(20.0, min(freq, sr * 0.48)) / sr
        alpha = math.sin(w0) / (2.0 * q)
        cosw = math.cos(w0)
        b0 = (1.0 - cosw) * 0.5
        b1 = 1.0 - cosw
        b2 = (1.0 - cosw) * 0.5
        a0 = 1.0 + alpha
        return Biquad(b0 / a0, b1 / a0, b2 / a0, (-2.0 * cosw) / a0, (1.0 - alpha) / a0)

    @staticmethod
    def highpass(freq: float, q: float = 0.707, sr: int = SAMPLE_RATE) -> "Biquad":
        w0 = 2.0 * math.pi * max(20.0, min(freq, sr * 0.48)) / sr
        alpha = math.sin(w0) / (2.0 * q)
        cosw = math.cos(w0)
        b0 = (1.0 + cosw) * 0.5
        b1 = -(1.0 + cosw)
        b2 = (1.0 + cosw) * 0.5
        a0 = 1.0 + alpha
        return Biquad(b0 / a0, b1 / a0, b2 / a0, (-2.0 * cosw) / a0, (1.0 - alpha) / a0)

    @staticmethod
    def bandpass(freq: float, q: float = 1.5, sr: int = SAMPLE_RATE) -> "Biquad":
        w0 = 2.0 * math.pi * max(20.0, min(freq, sr * 0.48)) / sr
        alpha = math.sin(w0) / (2.0 * q)
        cosw = math.cos(w0)
        b0 = alpha
        b1 = 0.0
        b2 = -alpha
        a0 = 1.0 + alpha
        return Biquad(b0 / a0, b1 / a0, b2 / a0, (-2.0 * cosw) / a0, (1.0 - alpha) / a0)


def make_seamless_loop(samples: List[float], crossfade_s: float = 0.15) -> List[float]:
    """Crossfade the tail of a buffer into its head so it loops with zero click."""
    fade_n = int(crossfade_s * SAMPLE_RATE)
    if len(samples) <= fade_n * 2:
        return samples
    n = len(samples) - fade_n
    out = list(samples[:n])
    for i in range(fade_n):
        t = i / fade_n
        # Equal-power-ish linear blend of tail into start
        out[i] = out[i] * t + samples[n + i] * (1.0 - t)
    return out


# ---------------------------------------------------------------------------
# Foley SFX generators
# ---------------------------------------------------------------------------

def gen_engine_loop() -> List[float]:
    """2.0s seamless fighter jet turbine + airframe hum loop."""
    rng = random.Random(101)
    dur = 2.15  # 0.15s crossfade -> 2.0s loop
    n = int(dur * SAMPLE_RATE)
    noise = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    rumble = Biquad.lowpass(180.0, 0.9).process(noise)
    air = Biquad.bandpass(620.0, 1.4).process([rng.uniform(-1.0, 1.0) for _ in range(n)])

    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        # Integer frequencies of 0.5 Hz period so 2.0s loop aligns in phase
        f0 = 55.0
        harm = (
            0.35 * math.sin(2.0 * math.pi * f0 * t)
            + 0.22 * math.sin(2.0 * math.pi * (f0 * 2.0) * t + 0.4)
            + 0.12 * math.sin(2.0 * math.pi * (f0 * 3.0) * t + 1.1)
            + 0.06 * math.sin(2.0 * math.pi * 1320.0 * t)  # turbine whine
            + 0.03 * math.sin(2.0 * math.pi * 2640.0 * t)
        )
        s = harm + 0.42 * rumble[i] + 0.20 * air[i]
        out.append(math.tanh(s * 1.3) * 0.75)
    return make_seamless_loop(out, 0.15)


def gen_boost_ignite() -> List[float]:
    """1.1s afterburner ignition thump and roar."""
    rng = random.Random(102)
    dur = 1.1
    n = int(dur * SAMPLE_RATE)
    out = []
    phase = 0.0
    lp_state = 0.0
    for i in range(n):
        t = i / SAMPLE_RATE
        # Ignition thump (95 Hz -> 32 Hz)
        freq = 32.0 + 63.0 * math.exp(-t * 14.0)
        phase += 2.0 * math.pi * freq / SAMPLE_RATE
        thump = math.sin(phase) * math.exp(-t * 6.5) * 0.8

        # Roar envelope rises fast, sustains, and tapers smoothly
        env = min(1.0, t / 0.04) * math.exp(-max(0.0, t - 0.25) * 1.8)
        cutoff = min(0.65, 0.08 + 0.45 * min(1.0, t / 0.18))
        raw_n = rng.uniform(-1.0, 1.0)
        lp_state += cutoff * (raw_n - lp_state)
        s = thump + lp_state * env * 0.95
        out.append(math.tanh(s * 1.6) * 0.85)
    return out


def gen_gun_shot() -> List[float]:
    """0.22s punchy aircraft autocannon shot."""
    rng = random.Random(103)
    dur = 0.22
    n = int(dur * SAMPLE_RATE)
    out = []
    phase = 0.0
    bp = Biquad.bandpass(1100.0, 1.1)
    lp = Biquad.lowpass(650.0, 0.8)
    for i in range(n):
        t = i / SAMPLE_RATE
        # Rapid pitch drop 280 Hz -> 52 Hz
        freq = 52.0 + 228.0 * math.exp(-t * 55.0)
        phase += 2.0 * math.pi * freq / SAMPLE_RATE
        body = math.sin(phase) * math.exp(-t * 28.0)

        # Muzzle blast noise
        blast_env = math.exp(-t * 34.0)
        noise = rng.uniform(-1.0, 1.0)
        blast = lp.step(noise) * blast_env

        # Mechanical breech clack at t = 0.038s
        clack_env = math.exp(-((t - 0.038) ** 2) / (2.0 * 0.004 ** 2))
        clack = bp.step(noise) * clack_env * 0.65

        s = body * 0.95 + blast * 0.85 + clack
        out.append(math.tanh(s * 2.1) * 0.88)
    return out


def gen_hit_metal() -> List[float]:
    """0.28s metallic airframe impact & ricochet ping."""
    rng = random.Random(104)
    dur = 0.28
    n = int(dur * SAMPLE_RATE)
    modes = [
        (780.0, 26.0, 0.45),
        (1340.0, 32.0, 0.35),
        (2180.0, 40.0, 0.28),
        (3520.0, 55.0, 0.20),
    ]
    hp = Biquad.highpass(500.0, 0.8)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        ring = sum(amp * math.sin(2.0 * math.pi * f * t) * math.exp(-t * d) for f, d, amp in modes)
        thud = math.sin(2.0 * math.pi * (120.0 * math.exp(-t * 18.0)) * t) * math.exp(-t * 35.0) * 0.7
        snap = hp.step(rng.uniform(-1.0, 1.0)) * math.exp(-t * 70.0) * 0.9
        out.append(math.tanh((ring + thud + snap) * 1.8) * 0.85)
    return out


def gen_part_break() -> List[float]:
    """0.75s structural airframe snap, tearing metal rivets, and wind flutter."""
    rng = random.Random(105)
    dur = 0.75
    n = int(dur * SAMPLE_RATE)
    rivet_times = [0.0, 0.045, 0.095, 0.14, 0.21]
    bp_tear = Biquad.bandpass(950.0, 1.6)
    lp_wind = Biquad.lowpass(420.0, 0.9)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        # Staggered rivet snaps
        snaps = 0.0
        for idx, rt in enumerate(rivet_times):
            if t >= rt:
                dt = t - rt
                f = 950.0 + idx * 210.0
                snaps += (
                    (math.sin(2.0 * math.pi * f * dt) + 0.7 * rng.uniform(-1.0, 1.0))
                    * math.exp(-dt * 48.0)
                    * 0.55
                )
        # Groaning metal twist (FM screech)
        fm_env = math.exp(-max(0.0, t - 0.03) * 5.5) * min(1.0, t / 0.02)
        mod = math.sin(2.0 * math.pi * 68.0 * t) * 4.0
        screech = math.sin(2.0 * math.pi * (540.0 - 220.0 * t) * t + mod) * fm_env * 0.45
        tear_noise = bp_tear.step(rng.uniform(-1.0, 1.0)) * fm_env * 0.5

        # Tumbling debris wind flutter
        flutter_env = math.exp(-t * 2.8) * (0.5 + 0.5 * math.sin(2.0 * math.pi * 16.0 * t))
        flutter = lp_wind.step(rng.uniform(-1.0, 1.0)) * flutter_env * 0.5

        out.append(math.tanh((snaps + screech + tear_noise + flutter) * 1.7) * 0.88)
    return out


def gen_lock_tick() -> List[float]:
    """0.14s missile seeker acquisition chirp/pulse."""
    dur = 0.14
    n = int(dur * SAMPLE_RATE)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        env = math.sin(math.pi * min(1.0, t / 0.11)) if t < 0.11 else 0.0
        rough = 0.7 + 0.3 * math.sin(2.0 * math.pi * 55.0 * t)
        sig = (
            0.6 * math.sin(2.0 * math.pi * 580.0 * t)
            + 0.4 * math.sin(2.0 * math.pi * 870.0 * t)
        )
        out.append(math.tanh(sig * rough * env * 1.8) * 0.65)
    return out


def gen_lock_tone() -> List[float]:
    """0.60s seamless high-urgency missile lock-on tone."""
    dur = 0.60
    n = int(dur * SAMPLE_RATE)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        # All frequencies are integer multiples of 1 / 0.60 = 1.6667 Hz (e.g. 10 Hz, 1100 Hz, 1450 Hz)
        am = 0.78 + 0.22 * math.sin(2.0 * math.pi * 15.0 * t)
        sig = (
            0.55 * math.sin(2.0 * math.pi * 1100.0 * t)
            + 0.45 * math.sin(2.0 * math.pi * 1450.0 * t)
            + 0.15 * math.sin(2.0 * math.pi * 2200.0 * t)
        )
        out.append(sig * am * 0.65)
    return out


def gen_missile_launch() -> List[float]:
    """1.20s rail release clunk + rocket motor ignition whoosh."""
    rng = random.Random(108)
    dur = 1.20
    n = int(dur * SAMPLE_RATE)
    out = []
    lp = 0.0
    hp_prev = 0.0
    for i in range(n):
        t = i / SAMPLE_RATE
        # Rail clunk at t=0
        clunk = (
            math.sin(2.0 * math.pi * (160.0 * math.exp(-t * 25.0)) * t)
            * math.exp(-t * 32.0)
            * 0.85
        )
        # Rocket motor whoosh (peaks around 0.18s, sweeps bandpass down as missile pulls away)
        whoosh_env = (1.0 - math.exp(-max(0.0, t - 0.03) * 18.0)) * math.exp(-max(0.0, t - 0.16) * 2.6)
        raw = rng.uniform(-1.0, 1.0)
        # Dynamic low-pass + high-pass
        alpha = max(0.08, min(0.75, 0.65 - 0.42 * (t / dur)))
        lp += alpha * (raw - lp)
        rocket = (lp - 0.35 * hp_prev) * whoosh_env
        hp_prev = lp

        # High turbine hiss
        hiss = raw * whoosh_env * 0.22
        out.append(math.tanh((clunk + rocket * 1.3 + hiss) * 1.6) * 0.88)
    return out


def gen_missile_burst() -> List[float]:
    """1.10s proximity warhead detonation boom + shrapnel crack."""
    rng = random.Random(109)
    dur = 1.10
    n = int(dur * SAMPLE_RATE)
    lp_boom = Biquad.lowpass(210.0, 0.8)
    bp_crack = Biquad.bandpass(1600.0, 1.2)
    out = []
    phase = 0.0
    for i in range(n):
        t = i / SAMPLE_RATE
        # Concussive sub wave (85 Hz -> 26 Hz)
        freq = 26.0 + 59.0 * math.exp(-t * 9.0)
        phase += 2.0 * math.pi * freq / SAMPLE_RATE
        sub = math.sin(phase) * math.exp(-t * 3.8)

        noise = rng.uniform(-1.0, 1.0)
        boom = lp_boom.step(noise) * math.exp(-t * 3.2)
        crack = bp_crack.step(noise) * math.exp(-t * 22.0)

        out.append(math.tanh((sub * 0.95 + boom * 1.1 + crack * 0.85) * 1.9) * 0.90)
    return out


def gen_plane_kill() -> List[float]:
    """1.60s airframe destruction: initial blast + secondary fuel fireball + debris."""
    rng = random.Random(110)
    dur = 1.60
    n = int(dur * SAMPLE_RATE)
    lp_rumble = Biquad.lowpass(175.0, 0.75)
    bp_mid = Biquad.bandpass(520.0, 0.9)
    out = []
    p1 = 0.0
    p2 = 0.0
    for i in range(n):
        t = i / SAMPLE_RATE
        # Primary impact
        f1 = 28.0 + 60.0 * math.exp(-t * 10.0)
        p1 += 2.0 * math.pi * f1 / SAMPLE_RATE
        sub1 = math.sin(p1) * math.exp(-t * 4.5)

        # Secondary fuel tank detonation at t = 0.14s
        t2 = max(0.0, t - 0.14)
        e2 = (1.0 if t >= 0.14 else 0.0) * math.exp(-t2 * 2.4)
        f2 = 24.0 + 50.0 * math.exp(-t2 * 7.0)
        p2 += 2.0 * math.pi * f2 / SAMPLE_RATE
        sub2 = math.sin(p2) * e2

        noise = rng.uniform(-1.0, 1.0)
        rumble = lp_rumble.step(noise) * (math.exp(-t * 2.5) + 0.9 * e2)
        crackle = bp_mid.step(noise) * (math.exp(-t * 8.0) + 0.7 * math.exp(-t2 * 6.0) * (1.0 if t >= 0.14 else 0.0))

        s = sub1 * 0.8 + sub2 * 0.95 + rumble * 1.1 + crackle * 0.65
        out.append(math.tanh(s * 1.8) * 0.92)
    return out


def gen_wind_rush() -> List[float]:
    """1.50s seamless high-G airflow & wing buffet loop."""
    rng = random.Random(111)
    dur = 1.65  # 0.15s crossfade -> 1.50s loop
    n = int(dur * SAMPLE_RATE)
    bp1 = Biquad.bandpass(290.0, 1.2)
    bp2 = Biquad.bandpass(780.0, 1.5)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        noise = rng.uniform(-1.0, 1.0)
        # Integer cycles over 1.50s (period 1.5s -> 2/3 Hz fundamental)
        gust = 0.72 + 0.28 * math.sin(2.0 * math.pi * (2.0 / 1.5) * t)
        s = (0.7 * bp1.step(noise) + 0.35 * bp2.step(noise)) * gust
        out.append(math.tanh(s * 1.4) * 0.70)
    return make_seamless_loop(out, 0.15)


def gen_ui_click() -> List[float]:
    """0.06s tactile cockpit toggle switch click."""
    rng = random.Random(112)
    dur = 0.06
    n = int(dur * SAMPLE_RATE)
    out = []
    for i in range(n):
        t = i / SAMPLE_RATE
        c1 = math.sin(2.0 * math.pi * 1850.0 * t) * math.exp(-t * 160.0)
        t2 = max(0.0, t - 0.011)
        c2 = (1.0 if t >= 0.011 else 0.0) * math.sin(2.0 * math.pi * 2650.0 * t2) * math.exp(-t2 * 210.0)
        n_tick = rng.uniform(-1.0, 1.0) * math.exp(-t * 190.0) * 0.35
        out.append(math.tanh((c1 * 0.8 + c2 * 0.65 + n_tick) * 1.8) * 0.75)
    return out


FOLEY_SPECS: List[Tuple[str, str, bool, Callable[[], List[float]]]] = [
    ("engine_loop", "Continuous jet turbine and airframe hum loop", True, gen_engine_loop),
    ("boost_ignite", "Afterburner ignition thump and exhaust roar", False, gen_boost_ignite),
    ("gun_shot", "Forward autocannon single shot burst", False, gen_gun_shot),
    ("hit_metal", "Bullet impact and metallic airframe ping", False, gen_hit_metal),
    ("part_break", "Structural part failure (wing, tail, or engine breaking off)", False, gen_part_break),
    ("lock_tick", "Missile seeker tracking pulse while holding target in cone", False, gen_lock_tick),
    ("lock_tone", "Continuous high-urgency tone when 1.2s missile lock closes", True, gen_lock_tone),
    ("missile_launch", "Pylon rail release clunk and rocket motor whoosh", False, gen_missile_launch),
    ("missile_burst", "Missile proximity warhead detonation", False, gen_missile_burst),
    ("plane_kill", "Full plane destruction explosion and secondary fireball", False, gen_plane_kill),
    ("wind_rush", "High-G turn airflow and wing buffet loop", True, gen_wind_rush),
    ("ui_click", "Cockpit switch click for menus and HUD controls", False, gen_ui_click),
]


# ---------------------------------------------------------------------------
# Pilot / Radio Voice Callouts
# ---------------------------------------------------------------------------

VOICE_SPECS: List[dict] = [
    {
        "id": "fox_two",
        "event": "missile_launch",
        "voice": "Samantha",
        "rate": 195,
        "text": "Fox two! Missile away!",
    },
    {
        "id": "guns_guns_guns",
        "event": "gun_fire_burst",
        "voice": "Daniel",
        "rate": 205,
        "text": "Guns, guns, guns!",
    },
    {
        "id": "target_locked",
        "event": "lock_acquired",
        "voice": "Samantha",
        "rate": 190,
        "text": "Target locked.",
    },
    {
        "id": "missile_incoming",
        "event": "incoming_missile_warning",
        "voice": "Karen",
        "rate": 210,
        "text": "Missile incoming! Break, break!",
    },
    {
        "id": "splash_one",
        "event": "enemy_killed",
        "voice": "Daniel",
        "rate": 195,
        "text": "Splash one bandit!",
    },
    {
        "id": "taking_fire",
        "event": "own_plane_hit",
        "voice": "Samantha",
        "rate": 205,
        "text": "Taking fire! Check six!",
    },
    {
        "id": "wing_lost",
        "event": "part_broken_wing",
        "voice": "Karen",
        "rate": 200,
        "text": "Lost a wing! Fighting for control!",
    },
    {
        "id": "tail_lost",
        "event": "part_broken_tail",
        "voice": "Daniel",
        "rate": 198,
        "text": "Tail control is out!",
    },
    {
        "id": "engine_out",
        "event": "part_broken_engine",
        "voice": "Samantha",
        "rate": 195,
        "text": "Engine's dead! Losing thrust!",
    },
    {
        "id": "going_down",
        "event": "own_plane_killed",
        "voice": "Karen",
        "rate": 205,
        "text": "Mayday, mayday, going down!",
    },
    {
        "id": "back_in_the_air",
        "event": "respawn_protected",
        "voice": "Samantha",
        "rate": 190,
        "text": "Back in the air. Weapons hot.",
    },
    {
        "id": "match_start",
        "event": "match_started",
        "voice": "Daniel",
        "rate": 185,
        "text": "All flights, cleared to engage.",
    },
]


def trim_silence(samples: List[float], threshold: float = 0.012) -> List[float]:
    start = 0
    end = len(samples)
    while start < end and abs(samples[start]) < threshold:
        start += 1
    while end > start and abs(samples[end - 1]) < threshold:
        end -= 1
    pad = int(0.015 * SAMPLE_RATE)
    start = max(0, start - pad)
    end = min(len(samples), end + pad)
    return samples[start:end] if end > start else samples


def apply_cockpit_radio_filter(speech: List[float], seed: int) -> List[float]:
    """
    Transform clean TTS speech into an aviation VHF cockpit radio transmission:
    - Pre-emphasis + 380..3100 Hz bandpass
    - Quindar PTT intro pip + mic click
    - Soft RF saturation/compression + carrier static
    - VHF squelch tail burst at mic release
    """
    rng = random.Random(seed)
    trimmed = trim_silence(speech)

    # Normalize raw speech before radio compression
    peak = max((abs(s) for s in trimmed), default=1.0)
    normed = [s / peak for s in trimmed] if peak > 1e-5 else trimmed

    hp1 = Biquad.highpass(380.0, 0.85)
    hp2 = Biquad.highpass(420.0, 0.85)
    lp1 = Biquad.lowpass(3100.0, 0.85)
    lp2 = Biquad.lowpass(2900.0, 0.85)
    mid_boost = Biquad.bandpass(1450.0, 1.1)

    filtered_voice = []
    for s in normed:
        v = hp2.step(hp1.step(s))
        v = lp2.step(lp1.step(v))
        v = v + 0.55 * mid_boost.step(v)
        # Radio AGC / overdrive saturation
        v = math.tanh(v * 2.35) * 0.78
        # Subtle carrier static during transmission
        static = (rng.uniform(-1.0, 1.0) * 0.025)
        filtered_voice.append(v + static)

    # Re-filter after distortion to keep bandwidth strictly VHF radio
    post_hp = Biquad.highpass(340.0, 0.707)
    post_lp = Biquad.lowpass(3300.0, 0.707)
    filtered_voice = [post_lp.step(post_hp.step(s)) for s in filtered_voice]

    # 1. PTT intro click + short 2475 Hz quindar tone (45 ms)
    intro_n = int(0.045 * SAMPLE_RATE)
    intro = []
    for i in range(intro_n):
        t = i / SAMPLE_RATE
        click = rng.uniform(-1.0, 1.0) * math.exp(-t * 180.0) * 0.55
        pip = math.sin(2.0 * math.pi * 2475.0 * t) * (0.22 if 0.006 <= t <= 0.036 else 0.0)
        intro.append(click + pip)

    # 2. Squelch tail burst at end (65 ms)
    tail_n = int(0.065 * SAMPLE_RATE)
    sq_bp = Biquad.bandpass(1650.0, 0.9)
    tail = []
    for i in range(tail_n):
        t = i / SAMPLE_RATE
        env = math.sin(math.pi * (i / tail_n)) * math.exp(-t * 14.0)
        click = rng.uniform(-1.0, 1.0) * math.exp(-t * 140.0) * 0.45
        noise = sq_bp.step(rng.uniform(-1.0, 1.0)) * env * 0.50
        tail.append(click + noise)

    return intro + filtered_voice + tail


def generate_foley() -> List[dict]:
    records = []
    for name, desc, loop, fn in FOLEY_SPECS:
        samples = fn()
        info = write_wav(FOLEY_DIR / f"{name}.wav", samples)
        info["id"] = name
        info["category"] = "foley"
        info["loop"] = loop
        info["description"] = desc
        records.append(info)
        print(f"[foley] {info['file']} ({info['duration_s']}s, {info['bytes']} B)")
    return records


def generate_tts_raw() -> None:
    RAW_TTS_DIR.mkdir(parents=True, exist_ok=True)
    for spec in VOICE_SPECS:
        raw_path = RAW_TTS_DIR / f"{spec['id']}.wav"
        cmd = [
            "/usr/bin/say",
            "-v",
            spec["voice"],
            "-r",
            str(spec["rate"]),
            "-o",
            str(raw_path),
            f"--data-format=LEI16@{SAMPLE_RATE}",
            spec["text"],
        ]
        subprocess.run(cmd, check=True)
        with wave.open(str(raw_path), "rb") as wf:
            frames = wf.getnframes()
        if frames == 0:
            raise RuntimeError(f"macOS 'say' produced 0 audio frames for {raw_path.name} (speechsynthesisd blocked by sandbox)")
        print(f"[tts-raw] {raw_path.name} ({frames} frames, {raw_path.stat().st_size} B)")


def generate_voice_filtered() -> List[dict]:
    records = []
    for idx, spec in enumerate(VOICE_SPECS):
        raw_path = RAW_TTS_DIR / f"{spec['id']}.wav"
        if not raw_path.exists():
            raise FileNotFoundError(f"Missing raw TTS file {raw_path}; run with --tts-raw first.")
        samples, sr = read_wav_mono(raw_path)
        if not samples:
            raise RuntimeError(f"Raw TTS file {raw_path} has 0 frames (was 'say' blocked by sandbox?).")
        processed = apply_cockpit_radio_filter(samples, seed=200 + idx)
        info = write_wav(VOICE_DIR / f"{spec['id']}.wav", processed, sr)
        info["id"] = spec["id"]
        info["category"] = "voice"
        info["event"] = spec["event"]
        info["voice"] = spec["voice"]
        info["transcript"] = spec["text"]
        records.append(info)
        print(f"[voice] {info['file']} ({info['duration_s']}s, {info['bytes']} B) - \"{spec['text']}\"")
    return records


def write_manifest() -> None:
    manifest_path = OUT_DIR / "manifest.json"
    foley_items = []
    for name, desc, loop, _ in FOLEY_SPECS:
        p = FOLEY_DIR / f"{name}.wav"
        if p.exists():
            with wave.open(str(p), "rb") as wf:
                frames = wf.getnframes()
                sr = wf.getframerate()
            foley_items.append({
                "id": name,
                "file": f"foley/{name}.wav",
                "category": "foley",
                "loop": loop,
                "description": desc,
                "sample_rate": sr,
                "frames": frames,
                "duration_s": round(frames / sr, 3),
                "bytes": p.stat().st_size,
            })

    voice_items = []
    for spec in VOICE_SPECS:
        p = VOICE_DIR / f"{spec['id']}.wav"
        if p.exists():
            with wave.open(str(p), "rb") as wf:
                frames = wf.getnframes()
                sr = wf.getframerate()
            voice_items.append({
                "id": spec["id"],
                "file": f"voice/{spec['id']}.wav",
                "category": "voice",
                "event": spec["event"],
                "voice": spec["voice"],
                "transcript": spec["text"],
                "sample_rate": sr,
                "frames": frames,
                "duration_s": round(frames / sr, 3),
                "bytes": p.stat().st_size,
            })

    manifest = {
        "format": "PCM_16LE_MONO_WAV",
        "sample_rate": SAMPLE_RATE,
        "foley": foley_items,
        "voice": voice_items,
    }
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"[manifest] wrote {manifest_path.relative_to(OUT_DIR.parent.parent)} "
          f"({len(foley_items)} foley, {len(voice_items)} voice)")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate procedural foley and pilot voice assets.")
    parser.add_argument("--foley", action="store_true", help="Generate procedural foley SFX")
    parser.add_argument("--tts-raw", action="store_true", help="Synthesize raw TTS clips with macOS 'say'")
    parser.add_argument("--voice-filter", action="store_true", help="Apply radio filter to raw TTS clips")
    parser.add_argument("--all", action="store_true", help="Run all generation steps")
    args = parser.parse_args()

    run_all = args.all or not (args.foley or args.tts_raw or args.voice_filter)
    if run_all or args.foley:
        generate_foley()
    if run_all or args.tts_raw:
        generate_tts_raw()
    if run_all or args.voice_filter:
        generate_voice_filtered()
    if run_all or args.foley or args.voice_filter:
        write_manifest()


if __name__ == "__main__":
    main()
