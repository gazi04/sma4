"""Parses level object streams by executing the ROM's own object generators in Unicorn.

For every level and candidate tileset this replays the loader loop of FUN_08014f20:
set the per-record RAM variables, call the position helper (0x08014D24) and the bank
dispatcher, then read back how far the level data pointer (0x03003B04) moved. That gives
exact per-object sizes without guessing from decompiled text.
Usage:
    tools/.venv/bin/python -I tools/analysis/emulate_levels.py <rom.gba> <out.json>
"""

import collections
import json
import struct
import sys
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_INTR, UC_MODE_THUMB, Uc, UcError
from unicorn.arm_const import (UC_ARM_REG_CPSR, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0,
                               UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP)

ROM_BASE = 0x08000000
LEVEL_TABLE, LEVEL_COUNT = 0x08136200, 581
HEADER_LEN = 11

# Loader RAM variables (see docs/re-notes.md).
V_DATA_PTR = 0x03003B04
V_BANK = 0x03003847
V_PARAM = 0x03003846
V_Y = 0x03003AD0
V_X = 0x03003AD1
V_ID = 0x03003848
V_TILESET = 0x03003849
V_VERTICAL = 0x03003CB3
V_LEVEL_STATE = 0x03007818  # pointer to a large level-state struct

FN_POSITION = 0x08014D24
FN_BANK0 = 0x08014A20
FN_BANK1 = 0x080149F8

STOP = 0x0203F000          # return address that ends a call
SCRATCH_STATE = 0x0203A000  # fake level-state struct (clear of the page buffers)
PAGE_BUFFERS = 0x0202E000   # 16 pages x 0x400 bytes of metatile ids (table 0x08136134)
STACK_TOP = 0x03007F00
MAX_INSNS = 2_000_000


def lz77(src):
    size = int.from_bytes(src[1:4], "little")
    out, i = bytearray(), 4
    while len(out) < size:
        flags = src[i]
        i += 1
        for bit in range(7, -1, -1):
            if len(out) >= size:
                break
            if flags >> bit & 1:
                n, d = (src[i] >> 4) + 3, ((src[i] & 0xF) << 8 | src[i + 1]) + 1
                i += 2
                for _ in range(n):
                    out.append(out[-d])
            else:
                out.append(src[i])
                i += 1
    return bytes(out[:size])


def rle(src):
    size = int.from_bytes(src[1:4], "little")
    out, i = bytearray(), 4
    while len(out) < size:
        f = src[i]
        i += 1
        if f & 0x80:
            out += bytes([src[i]]) * ((f & 0x7F) + 3)
            i += 1
        else:
            out += src[i:i + (f & 0x7F) + 1]
            i += (f & 0x7F) + 1
    return bytes(out[:size])


class Machine:
    def __init__(self, rom):
        self.rom = rom
        uc = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
        for base, size in ((0x00000000, 0x4000), (0x02000000, 0x40000), (0x03000000, 0x8000),
                           (0x04000000, 0x1000), (0x05000000, 0x1000), (0x06000000, 0x18000),
                           (0x07000000, 0x1000), (0x0E000000, 0x10000)):
            uc.mem_map(base, size)
        uc.mem_map(ROM_BASE, (len(rom) + 0xFFF) & ~0xFFF)
        uc.mem_write(ROM_BASE, rom)
        # Null generator entries jump to address 0 and slide to 0x08; "movs pc, lr" returns.
        uc.mem_write(0x08, struct.pack("<I", 0xE1B0F00E))
        uc.hook_add(UC_HOOK_INTR, self._swi)
        self.uc = uc
        self.swi_seen = collections.Counter()

    def _swi(self, uc, intno, _):
        """High-level BIOS: Unicorn has already stepped past the Thumb "svc #n"."""
        pc = uc.reg_read(UC_ARM_REG_PC)
        num = uc.mem_read(pc - 2, 1)[0]
        self.swi_seen[num] += 1
        r0, r1, r2 = (uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2))
        if num == 0x06:  # Div
            a = struct.unpack("<i", struct.pack("<I", r0))[0]
            b = struct.unpack("<i", struct.pack("<I", r1))[0] or 1
            q = int(a / b)
            uc.reg_write(UC_ARM_REG_R0, q & 0xFFFFFFFF)
            uc.reg_write(UC_ARM_REG_R1, (a - q * b) & 0xFFFFFFFF)
            uc.reg_write(UC_ARM_REG_R3, abs(q) & 0xFFFFFFFF)
        elif num in (0x0B, 0x0C):  # CpuSet / CpuFastSet
            count = r2 & 0x1FFFFF
            unit = 4 if (num == 0x0C or r2 & (1 << 26)) else 2
            if r2 & (1 << 24):
                uc.mem_write(r1, bytes(uc.mem_read(r0, unit)) * count)
            else:
                uc.mem_write(r1, bytes(uc.mem_read(r0, unit * count)))
        elif num in (0x11, 0x12, 0x14, 0x15):  # LZ77 / RLE decompress
            head = bytes(uc.mem_read(r0, 4))
            size = int.from_bytes(head[1:4], "little")
            src = bytes(uc.mem_read(r0, min(size * 2 + 0x1000, ROM_BASE + len(self.rom) - r0)
                                     if r0 >= ROM_BASE else size * 2 + 0x1000))
            uc.mem_write(r1, (lz77 if num < 0x13 else rle)(src))
        else:
            raise RuntimeError(f"unhandled SWI {num:#x}")

    def reset_ram(self):
        uc = self.uc
        uc.mem_write(0x02000000, bytes(0x40000))
        uc.mem_write(0x03000000, bytes(0x8000))
        self.w32(V_LEVEL_STATE, SCRATCH_STATE)

    def w8(self, a, v):
        self.uc.mem_write(a, bytes([v & 0xFF]))

    def w32(self, a, v):
        self.uc.mem_write(a, struct.pack("<I", v))

    def r32(self, a):
        return struct.unpack("<I", self.uc.mem_read(a, 4))[0]

    def call(self, fn, *args):
        uc = self.uc
        for reg, val in zip((UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3), args):
            uc.reg_write(reg, val)
        # Mode first: SP is banked per mode, so setting it before the switch is lost.
        uc.reg_write(UC_ARM_REG_CPSR, 0x3F)  # system mode, Thumb
        uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
        uc.reg_write(UC_ARM_REG_LR, STOP | 1)
        uc.emu_start(fn | 1, STOP, count=MAX_INSNS)
        if uc.reg_read(UC_ARM_REG_PC) & ~1 != STOP:
            raise RuntimeError(f"did not return from {fn:#x}")


def parse_level(m, off, tileset, reset=True):
    """Returns (end_offset, records) or (None, reason)."""
    rom = m.rom
    if reset:
        m.reset_ram()
    m.w8(V_TILESET, tileset)
    m.w8(V_VERTICAL, rom[off + 6] & 0x10)
    i = off + HEADER_LEN
    if rom[i] != 0xFF and rom[i] & 0xC0 == 0xC0:
        i += 3
    records = []
    while i < len(rom):
        b = rom[i]
        if b == 0xFF:
            return i + 1, records
        bank = b & 0xC0
        rec = rom[i:i + 4]
        if bank == 0x80:
            records.append({"kind": "setting", "at": i - off, "bytes": rec.hex()})
            i += 4
            continue
        m.w8(V_BANK, bank)
        m.w8(V_PARAM, b & 0x3F)
        m.w8(V_Y, rec[1])
        m.w8(V_X, rec[2])
        m.w8(V_ID, rec[3])
        m.w8(V_TILESET, tileset)
        m.w32(V_DATA_PTR, ROM_BASE + i + 4)
        try:
            m.call(FN_POSITION)
            if bank < 0x40:
                m.call(FN_BANK0)
            else:
                m.w8(V_BANK, bank | 1)
                m.call(FN_BANK1)
        except (UcError, RuntimeError) as e:
            return None, f"record at +{i - off:#x} {rec.hex()}: {e}"
        new = m.r32(V_DATA_PTR) - ROM_BASE
        extra = new - (i + 4)
        if not 0 <= extra <= 8:
            return None, f"record at +{i - off:#x} {rec.hex()}: pointer moved {extra}"
        records.append({"kind": "object", "bank": bank >> 6, "id": rec[3], "extra": extra,
                        "bytes": rom[i:new].hex()})
        i = new
    return None, "ran off ROM"


def main():
    rom = Path(sys.argv[1]).read_bytes()
    m = Machine(rom)
    ptrs = [struct.unpack_from("<I", rom, LEVEL_TABLE - ROM_BASE + n * 4)[0] - ROM_BASE
            for n in range(LEVEL_COUNT)]
    starts = sorted(set(ptrs))
    nxt = {s: starts[k + 1] for k, s in enumerate(starts[:-1])}

    out, stats = {}, collections.Counter()
    sizes = collections.defaultdict(set)  # (tileset, bank, id) -> observed extras
    for n, off in enumerate(ptrs):
        fits, errors = [], {}
        for t in range(1, 18):
            end, recs = parse_level(m, off, t)
            if end is None:
                errors[t] = recs
                continue
            if end == nxt.get(off, end):
                fits.append(t)
                for r in recs:
                    if r["kind"] == "object":
                        sizes[(t, r["bank"], r["id"])].add(r["extra"])
        stats["exact" if fits else "fail"] += 1
        out[n] = {"rom": f"{off + ROM_BASE:#010x}", "tilesets": fits,
                  "first_error": next(iter(errors.values()), None) if not fits else None}
        if n % 50 == 0:
            print(f"{n}/{LEVEL_COUNT} {dict(stats)}", file=sys.stderr)

    inconsistent = {f"{k}": sorted(v) for k, v in sizes.items() if len(v) > 1}
    print(dict(stats), "swi:", dict(m.swi_seen), file=sys.stderr)
    print("objects whose size varied:", len(inconsistent), file=sys.stderr)
    Path(sys.argv[2]).write_text(json.dumps({"levels": out, "inconsistent": inconsistent}, indent=1))


if __name__ == "__main__":
    main()
