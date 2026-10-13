"""Runs the ROM's own head-hit code on a fake player under a block in World 1-1.
Usage: tools/.venv/bin/python -I tools/analysis/probe_block_hit.py <rom.gba> <tile_x> <tile_y> [form]
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import emulate_levels as E  # noqa: E402

PLAYER, HIT, OBJS = 0x0203A000, 0x0203B000, 0x02038000
FN_HEAD_HIT, FN_BLOCK_HIT = 0x080A12BC, 0x080A077C


def build(rom):
    m = E.Machine(rom)
    off = struct.unpack_from("<I", rom, 0x136200 + 0x22 * 4)[0] - E.ROM_BASE
    m.reset_ram()
    for i in range(0xF0):
        m.call(0x08014A48, 0x80, i)
    for i in range(0x340):
        m.call(0x08014B94, 0x80, i)
    E.parse_level(m, off, 1, reset=False)
    return m


def main():
    rom = Path(sys.argv[1]).read_bytes()
    tx, ty = int(sys.argv[2]), int(sys.argv[3])
    form = int(sys.argv[4]) if len(sys.argv) > 4 else 0
    m = build(rom)
    m.w32(0x03007818, PLAYER)
    m.w32(0x03007820, HIT)
    m.w32(0x03007848, OBJS)
    m.w8(0x03003849, 1)
    m.w8(0x030038C5, 0xFF)
    m.uc.mem_write(0x03003DCA, b"\x80\x80")  # set by the tileset setup
    px, py = tx * 16, ty * 16 - 4  # probe (idx 0x20) = (px + 4, py + 16): inside the block
    m.w32(PLAYER + 0, px << 8)
    m.w32(PLAYER + 4, py << 8)
    m.w8(PLAYER + 0x37, form)
    m.uc.mem_write(PLAYER + 0xCA, struct.pack("<H", 0x1B0))
    m.call(FN_HEAD_HIT)
    hit = bytes(m.uc.mem_read(HIT, 0x41))
    code = hit[3]
    print(f"tile {tx},{ty}: probe code(+0x3e)={hit[0x3E]:#x} item code(+3)={code:#x} "
          f"bump timer(+0x71)={m.uc.mem_read(PLAYER + 0x71, 1)[0]:#x}")
    if code:
        m.call(FN_BLOCK_HIT)
        for slot in range(8):
            o = OBJS + slot * 0x70
            otype = struct.unpack("<H", bytes(m.uc.mem_read(o + 0x18, 2)))[0]
            if otype:
                state, variant = m.uc.mem_read(o + 0x16, 1)[0], m.uc.mem_read(o + 0x35, 1)[0]
                x, y = struct.unpack("<ii", bytes(m.uc.mem_read(o, 4)) + bytes(m.uc.mem_read(o + 8, 4)))
                print(f"  object slot {slot}: type={otype:#x} state={state} variant={variant} "
                      f"x={x / 256:.1f} y={y / 256:.1f}")
        cell = E.PAGE_BUFFERS + (tx >> 4) * 0x400 + ty * 16 + (tx & 15)
        print(f"  cell now {m.uc.mem_read(cell, 1)[0]:#x}, bump queue type={m.uc.mem_read(0x03003CC3, 1)[0]}")
    print("  swi", dict(m.swi_seen))


if __name__ == "__main__":
    main()
