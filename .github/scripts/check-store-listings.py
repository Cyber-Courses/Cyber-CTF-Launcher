#!/usr/bin/env python3
"""Checks the app's JSON files and the Microsoft Store listing before they ship.

- Every `src/messages/**/*.json` and `store/listings/*.json` parses.
- Each Store listing keeps Partner Center's limits (a submission over them is refused at release
  time, after the build): at most 7 search terms of at most 30 characters, features of at most
  200 characters (20 at most), a short description of at most 270 characters, a description of
  at most 10000, release notes of at most 1500, screenshot captions of at most 200.
- Every file a listing names exists: screenshots are PNGs of at least 1366x768, the poster and
  the shared box art / tile are PNGs, and each trailer has its video and PNG thumbnail.

Standard library only. Run from the repository root: python3 .github/scripts/check-store-listings.py
"""

import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORE = ROOT / "store"
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"

errors: list[str] = []


def fail(where: Path | str, message: str) -> None:
    errors.append(f"{where}: {message}")


def load_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as e:
        fail(path.relative_to(ROOT), f"invalid JSON: {e}")
        return None


def png_size(path: Path) -> tuple[int, int] | None:
    """(width, height) from the PNG header, or None when the file isn't a PNG."""
    try:
        with path.open("rb") as f:
            head = f.read(24)
    except OSError:
        return None
    if len(head) < 24 or not head.startswith(PNG_MAGIC) or head[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", head[16:24])


def check_png(path: Path, min_size: tuple[int, int] | None = None) -> None:
    rel = path.relative_to(ROOT)
    if not path.is_file():
        fail(rel, "missing")
        return
    size = png_size(path)
    if size is None:
        fail(rel, "not a PNG")
    elif min_size and (size[0] < min_size[0] or size[1] < min_size[1]):
        fail(rel, f"{size[0]}x{size[1]}, the Store wants at least {min_size[0]}x{min_size[1]}")


def check_text(rel: Path, listing: dict, key: str, limit: int, required: bool = True) -> None:
    value = listing.get(key)
    if value is None:
        if required:
            fail(rel, f"`{key}` is missing")
        return
    if not isinstance(value, str):
        fail(rel, f"`{key}` must be a string")
    elif len(value) > limit:
        fail(rel, f"`{key}` is {len(value)} characters, the Store allows {limit}")


def check_list(rel: Path, listing: dict, key: str, max_items: int, max_len: int) -> None:
    items = listing.get(key)
    if not isinstance(items, list):
        fail(rel, f"`{key}` must be a list")
        return
    if len(items) > max_items:
        fail(rel, f"{len(items)} {key}, the Store allows {max_items}")
    for item in items:
        if not isinstance(item, str):
            fail(rel, f"`{key}` entries must be strings: {item!r}")
        elif len(item) > max_len:
            fail(rel, f"{key} entry is {len(item)} characters, the Store allows {max_len}: {item!r}")


def check_listing(path: Path, listing: dict) -> None:
    rel = path.relative_to(ROOT)
    lang = path.stem
    if not isinstance(listing, dict):
        fail(rel, "must be a JSON object")
        return
    check_text(rel, listing, "title", 256)
    check_text(rel, listing, "shortDescription", 270)
    check_text(rel, listing, "description", 10000)
    check_text(rel, listing, "releaseNotes", 1500, required=False)
    check_list(rel, listing, "keywords", 7, 30)
    check_list(rel, listing, "features", 20, 200)

    images = STORE / "images" / lang
    screenshots = listing.get("screenshots") or []
    if not screenshots:
        fail(rel, "no screenshots")
    for shot in screenshots:
        if not isinstance(shot, dict) or "file" not in shot:
            fail(rel, f"a screenshot needs a `file`: {shot!r}")
            continue
        check_png(images / shot["file"], (1366, 768))
        caption = shot.get("caption", "")
        if len(caption) > 200:
            fail(rel, f"screenshot caption is {len(caption)} characters, the Store allows 200: {caption!r}")
    if listing.get("poster"):
        check_png(images / listing["poster"])

    trailer = listing.get("trailer")
    if trailer:
        # A language without its own trailer folder gets the en-us one (store-submit.ps1).
        folder = STORE / "trailers" / lang
        if not folder.is_dir():
            folder = STORE / "trailers" / "en-us"
        video = folder / trailer.get("video", "")
        if not video.is_file() or video.stat().st_size == 0:
            fail(rel, f"trailer video {video.relative_to(ROOT)} is missing")
        check_png(folder / trailer.get("thumbnail", ""))


def main() -> int:
    messages = sorted((ROOT / "src" / "messages").rglob("*.json"))
    if not messages:
        fail("src/messages", "no message files found")
    for path in messages:
        load_json(path)

    listings = sorted((STORE / "listings").glob("*.json"))
    if not listings:
        fail("store/listings", "no listings found")
    for path in listings:
        data = load_json(path)
        if data is None:
            continue
        if path.name == "common.json":
            for key in ("boxArt", "tile300"):
                if key in data:
                    check_png(STORE / "images" / "common" / data[key])
            continue
        check_listing(path, data)

    for e in errors:
        print(f"::error::{e}")
    checked = len(messages) + len(listings)
    print(f"{checked} JSON files checked, {len(listings) - 1} Store listings, {len(errors)} problem(s).")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
