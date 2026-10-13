"""Runs an object draw routine on a fake object slot and prints the OAM entries it writes.
usage: probe_object_oam.py rom.gba type draw_fn [byte_offset=value ...] [g:addr=value ...]"""
import struct
import sys

from emulate_levels import Machine

OBJS = 0x02038000          # object slots (0x70 bytes each); slot 0 is ours
OAM_BUF = 0x03002DC8
SLOT = 16                  # OAM index handed to the object
X, Y = 64, 64


def run(rom, typ, fn, fields, globs):
    m = Machine(rom)
    m.reset_ram()
    m.w32(0x03007848, OBJS)
    m.w32(0x03007818, 0x0203A000)
    m.w32(0x03007820, 0x0203B000)
    m.uc.mem_write(OAM_BUF - 0x100, bytes([0xA0, 0, 0, 0, 0, 0, 0, 0]) * 0x60)
    o = OBJS
    m.w32(o + 0, X << 8)
    m.w32(o + 8, Y << 8)
    m.uc.mem_write(o + 0x18, struct.pack("<H", typ))
    m.w8(o + 0x16, 2)
    m.w8(o + 0x21, rom[0x0816A770 - 0x08000000 + typ] & 7)
    m.uc.mem_write(OBJS + 0xC96, struct.pack("<H", SLOT * 4))
    for off, v in fields.items():
        m.w8(o + off, v)
    for a, v in globs.items():
        m.w8(a, v)
    m.call(fn, o)
    out = []
    for i in range(-0x20, 0x40):
        a0, a1, a2, _ = struct.unpack("<4H", bytes(m.uc.mem_read(OAM_BUF + i * 8, 8)))
        if a0 & 0xFF != 0xA0:
            out.append((i, a0, a1, a2))
    return out


def main():
    rom = open(sys.argv[1], "rb").read()
    typ, fn = int(sys.argv[2], 0), int(sys.argv[3], 0)
    fields, globs = {}, {}
    for kv in sys.argv[4:]:
        k, v = kv.split("=")
        if k.startswith("g:"):
            globs[int(k[2:], 0)] = int(v, 0)
        else:
            fields[int(k, 0)] = int(v, 0)
    for i, a0, a1, a2 in run(rom, typ, fn, fields, globs):
        y, x = a0 & 0xFF, a1 & 0x1FF
        print(f"slot {i:3d} y={y:3d} x={x:3d} shape={a0 >> 14} size={a1 >> 14} hflip={a1 >> 12 & 1} "
              f"vflip={a1 >> 13 & 1} tile={a2 & 0x3FF:#x} pal={a2 >> 12} dis={a0 >> 9 & 1}")


if __name__ == "__main__":
    main()
