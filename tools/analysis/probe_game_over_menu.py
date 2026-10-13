"""Runs the after-game-over state 0x2C (0x08012458) and renders the map screen it builds
(BG layers per DISPCNT, plus OAM), to see the game-over menu."""
import struct
import sys
from PIL import Image
from emulate_levels import Machine

ROM, OUT = sys.argv[1], sys.argv[2]
rom = open(ROM, "rb").read()
m = Machine(rom)
m.reset_ram()
uc = m.uc
for ptr, val in ((0x03007818, 0x0203A000), (0x03007820, 0x0203B000), (0x03007824, 0x0203C000),
                 (0x03007848, 0x02038000)):
    m.w32(ptr, val)
m.w8(0x0300385E, 1)          # one player
uc.mem_write(0x03002A6A, struct.pack("<H", 0xFFFF))   # no lives left
uc.mem_write(0x03002DC8, bytes([0xA0, 0, 0, 0, 0, 0, 0, 0]) * 128)
m.call(0x08012458)
for fn in sys.argv[3:]:
    m.call(int(fn, 0))


def u16(a):
    return struct.unpack("<H", bytes(uc.mem_read(a, 2)))[0]


bgpal = bytes(uc.mem_read(0x02000000, 0x200))
objpal = bytes(uc.mem_read(0x02000200, 0x200))


def color(pal, p, i):
    v = struct.unpack_from("<H", pal, (p * 16 + i) * 2)[0]
    return ((v & 31) * 255 // 31, (v >> 5 & 31) * 255 // 31, (v >> 10 & 31) * 255 // 31, 255)


def layer(bg):
    cnt = u16(0x03002DA4 + bg * 2)
    chars = bytes(uc.mem_read(0x06000000 + ((cnt >> 2) & 3) * 0x4000, 0x8000))
    screen = 0x06000000 + ((cnt >> 8) & 0x1F) * 0x800
    w, h = [(32, 32), (64, 32), (32, 64), (64, 64)][cnt >> 14]
    img = Image.new("RGBA", (w * 8, h * 8), (0, 0, 0, 0))
    for ty in range(h):
        for tx in range(w):
            block = tx // 32 + (ty // 32) * (w // 32)
            e = u16(screen + block * 0x800 + ((ty % 32) * 32 + tx % 32) * 2)
            t, hf, vf, p = e & 0x3FF, e >> 10 & 1, e >> 11 & 1, e >> 12
            d = chars[(t * 32) % len(chars):][:32]
            for y in range(8):
                for x in range(8):
                    sx, sy = (7 - x if hf else x), (7 - y if vf else y)
                    b = d[sy * 4 + sx // 2]
                    c = b >> 4 if sx & 1 else b & 15
                    if c:
                        img.putpixel((tx * 8 + x, ty * 8 + y), color(bgpal, p, c))
    return img, cnt


scroll = {0: (0x03003748, 0x0300374C), 1: (0x03003750, 0x03003754), 2: (0x03003758, 0x0300375C),
          3: (0x03003760, 0x03003764)}
disp = u16(0x03002D84)
screen = Image.new("RGBA", (240, 160), color(bgpal, 0, 0))
layers = [(u16(0x03002DA4 + bg * 2) & 3, bg) for bg in range(4) if disp >> (8 + bg) & 1]
for _, bg in sorted(layers, reverse=True):
    img, _ = layer(bg)
    hx, vy = u16(scroll[bg][0]), u16(scroll[bg][1])
    for y in range(160):
        for x in range(240):
            p = img.getpixel(((x + hx) % img.width, (y + vy) % img.height))
            if p[3]:
                screen.putpixel((x, y), p)
chars = bytes(uc.mem_read(0x06010000, 0x8000))
SIZES = [[(8, 8), (16, 16), (32, 32), (64, 64)], [(16, 8), (32, 8), (32, 16), (64, 32)],
         [(8, 16), (8, 32), (16, 32), (32, 64)]]
for i in range(127, -1, -1):
    a0, a1, a2 = struct.unpack("<3H", bytes(uc.mem_read(0x03002DC8 + i * 8, 6)))
    if a0 & 0xFF == 0xA0 or a0 >> 14 > 2:
        continue
    w, h = SIZES[a0 >> 14][a1 >> 14]
    y, x, t, p = a0 & 0xFF, a1 & 0x1FF, a2 & 0x3FF, a2 >> 12
    y = y - 256 if y >= 160 else y
    print("oam", i, x, y, (w, h), hex(t), p)
    for sy in range(h):
        for sx in range(w):
            tile = t + (sy // 8) * 32 + sx // 8
            d = chars[(tile * 32) % len(chars):][:32]
            b = d[(sy % 8) * 4 + (sx % 8) // 2]
            c = b >> 4 if sx & 1 else b & 15
            if c and 0 <= x + sx < 240 and 0 <= y + sy < 160:
                screen.putpixel((x + sx, y + sy), color(objpal, p, c))
screen.resize((720, 480), Image.NEAREST).save(f"{OUT}/go_menu.png")
print("disp", hex(disp), "state", hex(uc.mem_read(0x03003774, 1)[0]))
