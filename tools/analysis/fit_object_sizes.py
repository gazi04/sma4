"""Fills in object sizes the static scan missed by fitting them to the level data.

Starting from the statically derived table, greedily applies the single (table, id, extra)
override that makes the most additional levels parse exactly, until nothing improves.
Writes the final table and reports every override so each can be checked in Ghidra.
Usage:
    python3 -I tools/analysis/fit_object_sizes.py <rom.gba> <extra_bytes.json> <out.json>
"""

import collections
import json
import struct
import sys
from pathlib import Path

ROM_BASE = 0x08000000
LEVEL_TABLE, LEVEL_COUNT = 0x08136200, 581
HEADER_LEN = 11
# Tilesets 2-14 send IDs at or above these thresholds to tileset 1's tables.
SHARED_FROM = {0: 0x43, 1: 0x51}
OWN_TABLE_ONLY = {1, 15, 16, 17}


def table_key(bank, tileset, oid):
    if tileset not in OWN_TABLE_ONLY and oid >= SHARED_FROM[bank]:
        return f"{bank}:1"
    return f"{bank}:{tileset}"


def parse(rom, off, sizes, tileset, trail=None):
    """Offset past the terminator, or None. `trail` collects (key, id) of parsed objects."""
    i = off + HEADER_LEN
    if rom[i] != 0xFF and rom[i] & 0xC0 == 0xC0:
        i += 3
    while i + 4 <= len(rom):
        b = rom[i]
        if b == 0xFF:
            return i + 1
        bank = b & 0xC0
        if bank == 0x80:
            i += 4
            continue
        bk = 0 if bank == 0 else 1
        oid = rom[i + 3]
        key = table_key(bk, tileset, oid)
        ids = sizes[key]
        if oid >= len(ids) or ids[oid] < 0:
            return None
        if trail is not None:
            trail.append((key, oid))
        i += 4 + ids[oid]
    return None


def main():
    rom = Path(sys.argv[1]).read_bytes()
    sizes = json.loads(Path(sys.argv[2]).read_text())
    ptrs = [struct.unpack_from("<I", rom, LEVEL_TABLE - ROM_BASE + n * 4)[0] - ROM_BASE
            for n in range(LEVEL_COUNT)]
    starts = sorted(set(ptrs))
    nxt = {s: starts[k + 1] for k, s in enumerate(starts[:-1])}

    def exact(off, sz):
        for t in range(1, 18):
            end = parse(rom, off, sz, t)
            if end is not None and end == nxt.get(off, end):
                return True
        return False

    overrides = []
    while True:
        failing = [off for off in set(ptrs) if not exact(off, sizes)]
        print(f"{len(failing)} level streams still failing", file=sys.stderr)
        # Candidates: objects parsed before the failure point in any tileset.
        cands = collections.Counter()
        for off in failing:
            for t in range(1, 18):
                trail = []
                parse(rom, off, sizes, t, trail)
                for key, oid in set(trail):
                    for e in (0, 1, 2, 3):
                        if sizes[key][oid] != e:
                            cands[(key, oid, e)] += 1
        best, best_gain = None, 0
        for (key, oid, e), _ in cands.most_common(400):
            trial = {k: list(v) for k, v in sizes.items()}
            trial[key][oid] = e
            gain = sum(exact(off, trial) for off in failing)
            # Never accept a change that breaks a level that already parses.
            if gain > best_gain:
                broken = sum(1 for off in set(ptrs) if off not in failing and not exact(off, trial))
                if broken == 0:
                    best, best_gain = (key, oid, e), gain
        if not best:
            break
        key, oid, e = best
        print(f"override {key} id {oid:#04x}: {sizes[key][oid]} -> {e} (+{best_gain})", file=sys.stderr)
        overrides.append({"table": key, "id": oid, "from": sizes[key][oid], "to": e, "gain": best_gain})
        sizes[key][oid] = e

    Path(sys.argv[3]).write_text(json.dumps({"sizes": sizes, "overrides": overrides}))


if __name__ == "__main__":
    main()
