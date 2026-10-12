"""Derives how many extra bytes each level object consumes, per tileset and bank.

The level loader reads 4 bytes per object; some generators then advance the level data
pointer (0x03003B04) further. This scans the Ghidra export for those advances, follows
call edges, and prints a table. Usage:
    python3 -I tools/analysis/object_extra_bytes.py <rom.gba> <ghidra export dir>
"""

import collections
import json
import re
import struct
import sys
from pathlib import Path

ROM_BASE = 0x08000000
MAX_DEPTH = int(__import__("os").environ.get("MAX_DEPTH", "4"))
BANK0_DISPATCH = 0x08135F84  # bank 0 objects, one dispatcher per tileset
BANK1_DISPATCH = 0x08135F38  # bank 1 objects
TILESETS = 18

ADVANCE = re.compile(r"DAT_03003b04 = DAT_03003b04 \+ (\d+);")
TABLE_REF = re.compile(r"PTR_\w+_([0-9a-f]{8})\)\[DAT_03003848\]")


def main():
    rom = Path(sys.argv[1]).read_bytes()
    export = Path(sys.argv[2])
    u32 = lambda a: struct.unpack_from("<I", rom, a - ROM_BASE)[0]

    src = {}
    for f in (export / "functions").glob("*.c"):
        src[int(f.name[:8], 16)] = f.read_text()
    callees = {}
    for line in (export / "functions.tsv").read_text().splitlines()[1:]:
        cols = line.split("\t")
        callees[int(cols[0], 16)] = [int(c, 16) for c in cols[5].split(",") if c]

    def direct(fn):
        return sum(int(n) for n in ADVANCE.findall(src.get(fn, "")))

    memo = {}

    def extra(fn, depth=0):
        if fn in memo:
            return memo[fn]
        memo[fn] = 0  # break recursion
        total = direct(fn)
        if depth < MAX_DEPTH:
            total += max((extra(c, depth + 1) for c in callees.get(fn, [])), default=0)
        memo[fn] = total
        return total

    def generator_table(dispatch_fn):
        m = TABLE_REF.search(src.get(dispatch_fn, ""))
        if not m:
            return []
        table = int(m.group(1), 16)
        # Tables have zero holes for unused IDs; stop at the first non-pointer word.
        gens = []
        for i in range(256):
            p = u32(table + i * 4)
            if p == 0:
                gens.append(None)
            elif p & 1 and ROM_BASE <= p < ROM_BASE + len(rom):
                gens.append(p & ~1)
            else:
                break
        while gens and gens[-1] is None:
            gens.pop()
        return gens

    result = {}
    for bank, dispatch in ((0, BANK0_DISPATCH), (1, BANK1_DISPATCH)):
        for t in range(TILESETS):
            gens = generator_table(u32(dispatch + t * 4) & ~1)
            # -1 marks an unused ID (no generator).
            result[f"{bank}:{t}"] = [-1 if g is None else extra(g) for g in gens]

    json.dump(result, sys.stdout)
    summary = collections.Counter(n for v in result.values() for n in v)
    print(f"\nextra-byte histogram: {dict(summary)}", file=sys.stderr)
    for k, v in result.items():
        print(k, len(v), "ids", {i: n for i, n in enumerate(v) if n}, file=sys.stderr)


if __name__ == "__main__":
    main()
