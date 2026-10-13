"""Renders every player pose by running the game's own player-tile DMA setup
(0x080A6D80 + 0x080A3EB0) and reading the 32x32 OBJ VRAM area it fills (tiles 0-3 of
OBJ rows 0-3, 2D mapping). Output stays in the scratch dir given on the command line."""
import sys
from PIL import Image

from emulate_levels import Machine

ROM = sys.argv[1]
OUT = sys.argv[2]
FORMS = [int(f) for f in sys.argv[3].split(",")] if len(sys.argv) > 3 else [0, 1]
POSES = 205
PLAYER, PSTRUCT = 0x0203A000, 0x0203B000

rom = open(ROM, "rb").read()
m = Machine(rom)
m.reset_ram()
m.w32(0x03007818, PLAYER)
m.w32(0x03007820, PSTRUCT)
PAL = sys.argv[4] if len(sys.argv) > 4 else None
if PAL:
    raw = open(PAL, "rb").read()[7 * 32:8 * 32]
    def c555(v):
        return ((v & 31) * 255 // 31, (v >> 5 & 31) * 255 // 31, (v >> 10 & 31) * 255 // 31, 255)
    grey = [(0, 0, 0, 0)] + [c555(int.from_bytes(raw[i * 2:i * 2 + 2], "little")) for i in range(1, 16)]
else:
    grey = [(0, 0, 0, 0)] + [(i * 17, i * 17, i * 17, 255) for i in range(1, 16)]
from PIL import ImageDraw

for form in FORMS:
    sheet = Image.new("RGBA", (16 * 34, ((POSES + 15) // 16) * 34), (60, 60, 100, 255))
    for p in range(POSES):
        m.w8(PLAYER + 0x38, p)
        m.w8(PLAYER + 0x37, form)
        m.w8(0x03002D77, 0)
        m.call(0x080A6D80)
        m.call(0x080A3EB0)
        im = Image.new("RGBA", (32, 32))
        for ty in range(4):
            row = bytes(m.uc.mem_read(0x06010000 + ty * 0x400, 4 * 32))
            for tx in range(4):
                t = row[tx * 32:tx * 32 + 32]
                for y in range(8):
                    for x in range(8):
                        b = t[y * 4 + x // 2]
                        c = b >> 4 if x & 1 else b & 15
                        im.putpixel((tx * 8 + x, ty * 8 + y), grey[c])
        sheet.alpha_composite(im, ((p % 16) * 34 + 1, (p // 16) * 34 + 1))
    big = sheet.resize((sheet.width * 3, sheet.height * 3), Image.NEAREST)
    d = ImageDraw.Draw(big)
    for p in range(POSES):
        d.text(((p % 16) * 102 + 2, (p // 16) * 102 + 2), f"{p:02x}", fill=(255, 255, 0, 255))
    big.save(f"{OUT}/poses_form{form}_c.png")
    sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST).save(f"{OUT}/poses_form{form}.png")
print("ok")
