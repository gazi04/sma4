"""Runs the world map like the game does: the map init (mode 1 sub-state 0x10,
0x0800BE48) and then the per-frame map loop (sub-state 0x11, 0x0800DE10) with scripted
keys, printing the map sub-state (0x0300385D) and Mario's map position (*0x03007824).
usage: probe_map_walk.py rom.gba [world] [script]   script like "-60 A1 -30 R1 -40"
"""
import struct
import sys

from emulate_levels import Machine

MAP = 0x0203C000
KEYS = {"A": 1, "B": 2, "S": 8, "R": 0x10, "L": 0x20, "U": 0x40, "D": 0x80}


def setup(rom, world):
    m = Machine(rom)
    m.reset_ram()
    for ptr, val in ((0x03007818, 0x0203A000), (0x03007820, 0x0203B000), (0x03007824, MAP),
                     (0x03007848, 0x02038000)):
        m.w32(ptr, val)
    m.w8(0x03002A69, world)
    # Map position at the start of a world (Y 0x40, X 0x20: the START tile).
    m.w8(0x030037A9, 0x40)
    m.w8(0x030037AD, 0x20)
    # Optional completion bits: MAP_CLEARED="col:bits,col:bits" (0x02001420[world * 0x80 + col]).
    import os
    for spec in filter(None, os.environ.get("MAP_CLEARED", "").split(",")):
        col, bits = spec.split(":")
        m.w8(0x02001420 + world * 0x80 + int(col, 0), int(bits, 0))
    m.call(0x0800BE48)
    return m


def u8(m, a):
    return m.uc.mem_read(a, 1)[0]


def u16(m, a):
    return struct.unpack("<H", bytes(m.uc.mem_read(a, 2)))[0]


def state(m):
    return dict(sub=u8(m, 0x03003774), map=u8(m, 0x0300385D), y=u8(m, MAP), xhi=u8(m, MAP + 2),
                x=u8(m, MAP + 4), walk=u8(m, MAP + 6), dir=u8(m, MAP + 8), tile=u8(m, 0x030038BA),
                scroll=u16(m, 0x03003750))


def run(m, script, verbose=True):
    held = 0
    frame = 0
    log = []
    for tok in script.split():
        keys, n = tok[:-len(tok.lstrip("ABSRLUD-"))] if False else ("", 0), 0
        i = 0
        while i < len(tok) and not tok[i].isdigit():
            i += 1
        keys, n = tok[:i], int(tok[i:])
        mask = 0
        for c in keys:
            mask |= KEYS.get(c, 0)
        for _ in range(n):
            pressed = mask & ~held
            held = mask
            m.uc.mem_write(0x03002DBC, struct.pack("<HH", held, pressed))
            sub = u8(m, 0x03003774)
            if sub != 0x11:
                print("left the map loop: sub-state", hex(sub))
                return log
            m.call(0x0800DE10)
            frame += 1
            # Global frame counter, advanced by the main loop.
            m.w8(0x03003AE8, (u8(m, 0x03003AE8) + 1) & 0xFF)
            s = state(m)
            log.append(s)
            if verbose and (len(log) < 2 or s != log[-2]):
                print(frame, {k: hex(v) for k, v in s.items()})
    return log


if __name__ == "__main__":
    rom = open(sys.argv[1], "rb").read()
    world = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    script = sys.argv[3] if len(sys.argv) > 3 else "-120 A1 -60 R1 -40 R1 -40"
    m = setup(rom, world)
    m.w8(0x03003774, 0x11)
    print("after init", {k: hex(v) for k, v in state(m).items()})
    run(m, script)
