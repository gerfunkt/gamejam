#!/usr/bin/env python3
"""
Scrape and verify authentic F-16 / F/A-18 "Bitching Betty" (Voice Warning System)
and F-16 combat SFX .wav assets from open-source flight simulator repositories,
store raw files in assets/audio/scraped/, copy runtime files into web/assets/audio/,
and emit web/assets/audio/audio_catalog.json mapped to crates/sim events.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import sys
import urllib.request
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "assets" / "audio" / "betty_scrape_manifest.json"
CATALOG_PATH = ROOT / "web" / "assets" / "audio" / "audio_catalog.json"


def git_blob_sha1(data: bytes) -> str:
    header = f"blob {len(data)}\0".encode("ascii")
    h = hashlib.sha1()
    h.update(header)
    h.update(data)
    return h.hexdigest()


def inspect_wav(path: Path) -> dict[str, int | float | str]:
    """Parse WAV header (supports PCM and IEEE float/extensible RIFF WAVE)."""
    data = path.read_bytes()
    if len(data) < 44 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"{path} is not a valid RIFF WAVE file")

    try:
        with wave.open(str(path), "rb") as wf:
            channels = wf.getnchannels()
            sample_width = wf.getsampwidth()
            sample_rate = wf.getframerate()
            frames = wf.getnframes()
            duration = round(frames / float(sample_rate), 3) if sample_rate else 0.0
            return {
                "format": "wav",
                "channels": channels,
                "sample_rate_hz": sample_rate,
                "bits_per_sample": sample_width * 8,
                "frames": frames,
                "duration_s": duration,
            }
    except wave.Error:
        # Fallback parser for non-PCM fmt chunks (e.g. IEEE float or extensible WAV)
        offset = 12
        channels = 1
        sample_rate = 44100
        byte_rate = 88200
        bits_per_sample = 16
        data_size = len(data) - 44
        while offset + 8 <= len(data):
            chunk_id = data[offset : offset + 4]
            chunk_size = struct.unpack("<I", data[offset + 4 : offset + 8])[0]
            chunk_start = offset + 8
            if chunk_id == b"fmt " and chunk_size >= 16:
                _, channels, sample_rate, byte_rate, _, bits_per_sample = struct.unpack(
                    "<HHIIHH", data[chunk_start : chunk_start + 16]
                )
            elif chunk_id == b"data":
                data_size = chunk_size
                break
            offset = chunk_start + chunk_size + (chunk_size % 2)
        duration = round(data_size / float(byte_rate), 3) if byte_rate else 0.0
        return {
            "format": "wav",
            "channels": channels,
            "sample_rate_hz": sample_rate,
            "bits_per_sample": bits_per_sample,
            "frames": int(duration * sample_rate),
            "duration_s": duration,
        }


def download(url: str) -> bytes:
    req = urllib.request.Request(
        url,
        headers={"User-Agent": "gamejam-betty-scraper/1.0"},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.read()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate manifest and print planned downloads without network requests.",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-download all files even if cached SHA-1 matches.",
    )
    args = parser.parse_args()

    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    assets = manifest["assets"]

    downloaded = 0
    cached = 0
    catalog_entries = []

    for item in assets:
        asset_id = item["id"]
        url = item["url"]
        expected_sha = item["git_sha1"]
        expected_size = item["size_bytes"]
        scraped_path = ROOT / item["scraped_path"]
        web_path = ROOT / item["web_path"]

        scraped_path.parent.mkdir(parents=True, exist_ok=True)
        web_path.parent.mkdir(parents=True, exist_ok=True)

        if args.dry_run:
            print(f"[DRY-RUN] {asset_id}: {url} -> {item['web_path']} ({expected_size} B)")
            continue

        need_download = args.force or not scraped_path.exists()
        if not need_download:
            existing_bytes = scraped_path.read_bytes()
            if git_blob_sha1(existing_bytes) == expected_sha:
                cached += 1
            else:
                need_download = True

        if need_download:
            raw = download(url)
            actual_sha = git_blob_sha1(raw)
            if actual_sha != expected_sha:
                print(
                    f"ERROR: SHA-1 mismatch for {asset_id}: expected {expected_sha}, got {actual_sha}",
                    file=sys.stderr,
                )
                return 1
            if len(raw) != expected_size:
                print(
                    f"ERROR: Size mismatch for {asset_id}: expected {expected_size}, got {len(raw)}",
                    file=sys.stderr,
                )
                return 1
            scraped_path.write_bytes(raw)
            downloaded += 1

        shutil.copy2(scraped_path, web_path)
        wav_meta = inspect_wav(web_path)

        rel_web_url = "/" + str(item["web_path"]).removeprefix("web/")
        catalog_entries.append(
            {
                "id": asset_id,
                "category": item["category"],
                "url": rel_web_url,
                "transcript": item["transcript"],
                "sim_trigger": item["sim_trigger"],
                "priority": item["priority"],
                "cooldown_s": item["cooldown_s"],
                "description": item["description"],
                "source": item["source"],
                "git_sha1": expected_sha,
                "size_bytes": expected_size,
                "audio": wav_meta,
            }
        )
        print(
            f"[OK] {asset_id:24s} -> {rel_web_url:42s} "
            f"({wav_meta['duration_s']:5.2f}s, {wav_meta['sample_rate_hz']} Hz, {expected_size} B)"
        )

    if args.dry_run:
        print(f"Dry run complete: {len(assets)} assets validated.")
        return 0

    catalog = {
        "version": 1,
        "total_assets": len(catalog_entries),
        "event_map": {
            "part_broken_master": "betty_warning",
            "part_broken_left_wing": ["betty_warning", "betty_aoa_over_limit", "sfx_stall_buffet"],
            "part_broken_right_wing": ["betty_warning", "betty_aoa_over_limit", "sfx_stall_buffet"],
            "part_broken_tail": ["betty_warning", "betty_max_aoa", "sfx_stall_buffet"],
            "part_broken_engine": ["betty_warning", "betty_min_speed", "sfx_stall_horn"],
            "fuselage_critical": ["betty_warning_double", "sfx_caution_tone"],
            "part_damaged_caution": ["betty_caution", "sfx_caution_tone"],
            "hit_taken": ["sfx_caution_tone"],
            "lock_tracking": ["sfx_aim9_growl"],
            "lock_acquired": ["betty_lock", "sfx_rwr_new"],
            "enemy_lock_on_me": ["sfx_rwr_sam_launch", "betty_lock"],
            "missile_launched_by_me": ["sfx_aim_launch", "sfx_missile_motor"],
            "missile_incoming": ["sfx_rwr_launch", "betty_missile"],
            "missile_evaded": ["betty_chaff_flare"],
            "missile_burst": ["sfx_missile_explosion"],
            "gun_fire": ["sfx_vulcan_cannon"],
            "plane_killed": ["sfx_missile_explosion"],
            "spawn_protected_start": ["betty_jammer"],
            "spawn_protected_end": ["betty_chaff_flare_out"],
            "target_is_protected": ["betty_iff"],
            "low_altitude": ["betty_altitude"],
            "steep_dive_pullup": ["betty_pullup_f16", "betty_pullup_wt"],
            "hard_turn_over_g": ["betty_over_g", "betty_g_over_limit"],
            "boost_max_speed": ["betty_max_speed", "sfx_exhaust_afterburner"],
            "engine_loop": ["sfx_engine_cockpit"],
            "match_start": ["betty_data", "betty_gear_up"],
            "match_end": ["betty_bingo", "betty_bingo_fuel", "betty_gear_down"],
        },
        "assets": catalog_entries,
    }

    CATALOG_PATH.parent.mkdir(parents=True, exist_ok=True)
    CATALOG_PATH.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    print(
        f"\nCompleted: {downloaded} downloaded, {cached} cached, "
        f"{len(catalog_entries)} written to {CATALOG_PATH.relative_to(ROOT)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
