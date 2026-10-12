"""Checks the object-size table by parsing every level stream.

A parse is "exact" when the stream ends on 0xFF and the next byte is where the next
level (in ROM order) starts. Usage:
    python3 -I tools/analysis/validate_levels.py <rom.gba> <extra_bytes.json>
"""

import collections
import json
import struct
import sys
from pathlib import Path

ROM_BASE = 0x08000000
LEVEL_TABLE, LEVEL_COUNT = 0x08136200, 581
HEADER_LEN = 11


def parse(rom, off, extra, tileset):
    """Returns the offset just past the 0xFF terminator, or None if the stream is invalid."""
    i = off + HEADER_LEN
    if rom[i] != 0xFF and rom[i] & 0xC0 == 0xC0:
        i += 3
    while i < len(rom):
        b = rom[i]
        if b == 0xFF:
            return i + 1
        bank = b & 0xC0
        if bank == 0x80:
            i += 4
            continue
        ids = extra[f"{0 if bank == 0 else 1}:{tileset}"]
        oid = rom[i + 3]
        if oid >= len(ids) or ids[oid] < 0:
            return None
        i += 4 + ids[oid]
    return None


def main():
    rom = Path(sys.argv[1]).read_bytes()
    extra = json.loads(Path(sys.argv[2]).read_text())
    ptrs = [struct.unpack_from("<I", rom, LEVEL_TABLE - ROM_BASE + n * 4)[0] for n in range(LEVEL_COUNT)]
    starts = sorted(set(p - ROM_BASE for p in ptrs))
    next_start = {s: starts[k + 1] for k, s in enumerate(starts[:-1])}

    stats = collections.Counter()
    fits = {}
    for n, p in enumerate(ptrs):
        off = p - ROM_BASE
        ok = []
        for t in range(18):
            end = parse(rom, off, extra, t)
            if end is not None and end == next_start.get(off, end):
                ok.append(t)
        fits[n] = ok
        stats["exact" if ok else "no fit"] += 1
    print(dict(stats))
    print("no-fit levels:", [hex(n) for n, ok in fits.items() if not ok][:40])
    by_ts = collections.Counter(tuple(ok) for ok in fits.values())
    print("most common candidate sets:", by_ts.most_common(12))


if __name__ == "__main__":
    main()
