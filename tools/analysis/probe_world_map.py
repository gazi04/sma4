"""Runs the world-map init (0x0800BE48 -> 0x0800B648) and renders the four BG layers it
sets up, to find out how the map screen is built. Output goes to the given directory."""
import struct
import sys
from PIL import Image
from emulate_levels import Machine

ROM, OUT = sys.argv[1], sys.argv[2]
WORLD = int(sys.argv[3]) if len(sys.argv) > 3 else 0
rom = open(ROM, "rb").read()
m = Machine(rom)
m.reset_ram()
uc = m.uc
for ptr, val in ((0x03007818, 0x0203A000), (0x03007820, 0x0203B000), (0x03007824, 0x0203C000),
                 (0x03007848, 0x02038000)):
    m.w32(ptr, val)
m.w8(0x03002A69, WORLD)
m.call(0x0800BE48)


def u16(a):
    return struct.unpack("<H", bytes(uc.mem_read(a, 2)))[0]


pal = bytes(uc.mem_read(0x05000000, 0x200))
if not any(pal):
    pal = bytes(uc.mem_read(0x02000000, 0x200))


def color(p, i):
    v = struct.unpack_from("<H", pal, (p * 16 + i) * 2)[0]
    return ((v & 31) * 255 // 31, (v >> 5 & 31) * 255 // 31, (v >> 10 & 31) * 255 // 31, 255)


for bg in range(4):
    cnt = u16(0x03002DA4 + bg * 2)
    char_base = 0x06000000 + ((cnt >> 2) & 3) * 0x4000
    screen = 0x06000000 + ((cnt >> 8) & 0x1F) * 0x800
    size = cnt >> 14
    w, h = [(32, 32), (64, 32), (32, 64), (64, 64)][size]
    img = Image.new("RGBA", (w * 8, h * 8), (0, 0, 0, 0))
    for ty in range(h):
        for tx in range(w):
            block = (tx // 32) + (ty // 32) * (2 if w == 64 else 1)
            e = u16(screen + block * 0x800 + ((ty % 32) * 32 + tx % 32) * 2)
            t, hf, vf, p = e & 0x3FF, e >> 10 & 1, e >> 11 & 1, e >> 12
            data = bytes(uc.mem_read(char_base + t * 32, 32))
            for y in range(8):
                for x in range(8):
                    sx, sy = (7 - x if hf else x), (7 - y if vf else y)
                    b = data[sy * 4 + sx // 2]
                    c = b >> 4 if sx & 1 else b & 15
                    if c:
                        img.putpixel((tx * 8 + x, ty * 8 + y), color(p, c))
    img.save(f"{OUT}/map_bg{bg}.png")
    print(bg, hex(cnt), w, h)
print("scroll", hex(u16(0x03003750)), hex(u16(0x03003754)), "dispcnt", hex(u16(0x03002D84)))

# Compose the 240x160 screen from the enabled BGs (no OBJ), like the GBA does.
disp = u16(0x03002D84)
scroll = {  # BGnHOFS / BGnVOFS shadows written in VBlank (0x0800092C, 0x08000DD0)
    0: (u16(0x03003748), u16(0x0300374C)),
    1: (u16(0x03003750), (u16(0x03003754) + u16(0x03003768)) & 0xFFFF),
    2: (u16(0x03003758), u16(0x0300375C)),
    3: (u16(0x03003760), u16(0x03003764)),
}
layers = {bg: Image.open(f"{OUT}/map_bg{bg}.png") for bg in range(4)}
screen = Image.new("RGBA", (240, 160), color(0, 0))
order = sorted((u16(0x03002DA4 + bg * 2) & 3, -bg, bg) for bg in range(4) if disp >> (8 + bg) & 1)
for _, _, bg in reversed(order):
    lw, lh = layers[bg].size
    hx, vy = scroll[bg]
    for y in range(160):
        for x in range(240):
            p = layers[bg].getpixel(((x + hx) % lw, (y + vy) % lh))
            if p[3]:
                screen.putpixel((x, y), p)
screen.resize((720, 480), Image.NEAREST).save(f"{OUT}/map_screen.png")
print("bgs", [b for *_, b in order], {k: tuple(hex(v) for v in s) for k, s in scroll.items()})
