# SMA4 port (Rust + Bevy)

A frame-accurate reimplementation of Super Mario Advance 4 in Rust, with Bevy as the host.
**No game data is included.** You need your own ROM dump: `sma4-extract` turns it into local
assets once (in `assets/extracted/`, git-ignored, never share it), and the game runs from those.

## Layout
| Path | Role |
|---|---|
| `crates/sma4-rom` | ROM access, BIOS decompressors (LZ77/Huffman/RLE), GBA tile/palette formats, level tables |
| `crates/sma4-extract` | **One-time extractor**: ROM → `assets/extracted/` (runs the ROM's own level loaders in Unicorn) |
| `crates/sma4-assets` | File formats shared by the extractor and the game |
| `crates/sma4-core` | Deterministic game logic, with no Bevy or floats. Stepped once per GBA frame |
| `crates/sma4-bevy` | The game: window, input, native rendering. **Reads only `assets/extracted/`, never the ROM** |
| `tools/ghidra` | Headless Ghidra import, analysis and export of decompiled C (output is git-ignored) |
| `tools/analysis` | Python reverse-engineering helpers (`emulate_levels.py` validates the level format) |
| `docs/re-notes.md` | Reverse-engineering findings |

## Run
```sh
# Once: extract assets from your own ROM (writes assets/extracted/, git-ignored, never share it)
cargo run --release -p sma4-extract -- path/to/sma4.gba

cargo run --release -p sma4-bevy             # play World 1-1
cargo run -p sma4-bevy -- --viewer           # browse the extracted graphics
cargo run -p sma4-bevy -- --script "-20 R120 RA30 R15" --screenshot out.png   # automated check
```
Re-run the extractor when it learns new assets. More levels: `--level <hex id>:<tileset>`.

On a display near 60 Hz the game runs one GBA frame per screen refresh, so scrolling is smooth
(the GBA's 59.73 Hz against a 60 Hz screen would otherwise freeze a frame every few seconds);
at other refresh rates it keeps the GBA's speed. The log says which (`display ... Hz`).
Scripts run one frame per rendered frame.

Ghidra:
```sh
tools/ghidra/run_headless.sh path/to/sma4.gba    # re-import + export
tools/ghidra/run_headless.sh --export            # re-export only
```

Supported ROM: Europe AX4P rev 0 (SHA1 `b7f3d6dc014e2f0ddc996137bdefbb8167a77e66`).

Controls: arrows = move, X = jump (A), Z = run (B), Enter = Start, Backspace = Select, A/S = L/R.

The item panel works as in the ROM (checked in mGBA): L on the map slides the status bar
away and raises the panel with your items (nine to a row); the pad moves the cursor (the
chosen item in colour), B or L closes it, A uses the item and closes it. Every item works
as in the ROM, and one that can't be used now is refused:
- A mushroom, fire flower, leaf, Frog, Tanooki or Hammer Suit changes Mario's form, on the
  map and into the next level, with a puff of smoke over him that holds the map for a
  moment.
- The cloud carries Mario past one level he hasn't cleared; the P-Wing makes him raccoon
  Mario with a P-meter that stays full for the next level. Both are gone once that level
  is over.
- The Starman makes the next level start with a star (refused if one is already waiting).
- The anchor is taken only while an airship waits on the map.
- The hammer breaks a rock next to Mario (with a puff) and opens the path behind it;
  without a rock it is refused.
- The Warp Whistle calls the whirlwind: it flies across the map and takes Mario away, the
  map waves and World 9, the Warp Zone, comes in, waving, until the whirlwind drops Mario
  there (timings recorded in mGBA).

The new forms play as in the ROM (checked frame by frame in mGBA): the Frog Suit hops on
land and swims freely in water (Up / Down / Left / Right, faster with A held); the Tanooki
Suit has the raccoon's tail and flight, and Down + B turns him to stone (no harm can come to
him) until Down is let go; the Hammer Suit throws hammers in an arc that fly through walls
and knock out any enemy they touch, even fire-proof ones. Losing a suit to a hit makes Mario
big. Not ported: the sounds, the palette flash, a hammer breaking bricks. Debug: `--items
1,2,3,9,c` starts with items (hex ids; in scripts `Q` is the L button), `--form 0-6`.

World 9 shows its "WORLD 9 WARP ZONE!" sign and the world numbers over its pipes, and each
pipe leads to its world's map (Worlds 2-8), which starts with its "WORLD n" panel and Mario
at its start, as in the ROM (checked in mGBA). Those worlds' levels are not ported:
entering one leaves you on the map. Worlds 2-8's maps and map objects are extracted from
your ROM (re-run `sma4-extract`).

World 1-5 works as in the ROM. Its pools are water: Mario swims in them. A strokes up
(speed -0x20 off the bottom, 0x20 more upward in the water, held within +/-0x20), the
water pulls him down slowly (+1 a frame rising, every other frame sinking) and pushes him
back once he rises over 0x14 px above the top of the screen, he swims across at up to 0x18
(8 walking on the bottom) with the ROM's water friction, and the stroke and paddling poses are extracted from
your ROM. Falling in stops the fall (and the motion across); at the surface he bobs and A
does nothing; Up + A jumps out. Going in or out splashes (six steps, extracted from your
ROM), diving in lets out three bubbles, and he breathes out a bubble every 0x80 frames
(0x40 while moving); bubbles rise, sway and pop at the surface. Buzzy Beetles and shells
that fall in splash too and sink slowly (gravity 1, at most 0x10, shells at half speed).
Swimming, the splash, the bubbles and the Buzzy Beetles and kicked shells in the pools
were checked frame by frame against the ROM's own routines run in an emulator.

The camera now scrolls up and down as in the ROM in every level (checked frame by frame
against the ROM's scroll routines over a run through World 1-5): it follows Mario only
when his frame's top leaves a band below the top of the view (0x10-0x30 px in World 1-5),
at a speed set by his vertical speed. In most levels, once it is down at the ground (y 0xF0
or below) it does not follow him up unless he flies; World 1-5's header picks the ROM's
free scroll instead, so the camera follows him back up onto the hills above the cave
(it used to stay down there with Mario off the top of the screen). Each level's settings
are extracted from your ROM (the level files now carry the scroll mode: re-run
`sma4-extract`). Views taller than the GBA's
show more above, with the bottom of the level kept at the bottom. World 1-5's camera
triggers work on it: over the note-block stretch (two 0xC8 boxes) the camera heads for
the bottom of the level, and near the end (an 0xE0 / 0xE1 band) it stays up while Mario is
above the band's row. Sliding shells keep the ROM's floor speed (0x0C), so they leave
ledges and drop into one-tile gaps as in the ROM.
Debug: `--level 82 --spawn-col 26` starts in the first pool.

Levels have the ROM's background scene behind them (BG2, 512x512, extracted from your ROM
into `levels/level_XXX_bg.png`): World 1-1's distant blocks and bushes, 1-2's tall hills.
It scrolls as in the ROM: half the camera's speed across, 3/4 down (+8). World 1-5's
sky area (through the hidden orange note block) has the ROM's clouds: the ROM unpacks that
scene's map from its own routine, so the port used to show one tile repeated over the whole
sky; the clouds scroll as in the ROM, 7/8 down (+0x20). Checked by playing into the room in
both the port and the real game (mGBA) through the hidden block at the top of the narrow
shaft (column 57). Not ported: 1-5's farther cave background (BG3, olive arches behind the
brown ones). Re-run `sma4-extract` (the level files now carry the scene mode).

World 1's Hammer Bro works as in the ROM. On the map he stands by the bridge, stepping
from side to side, until you lose a life: then Mario goes back to the last level he cleared
(as after any death) and the Hammer Bro walks off: a step or two along the paths, picking
his way with the game's own random bytes, never stopping on a level panel, a Toad House or
START. Meeting him on a path junction starts a battle at once (no iris) in a one-screen
arena: he walks to and fro, jumps up through a row of bricks and hops down again, and
throws spinning hammers that fly in an arc. Knock him out (stomp, fire, tail, star) and once
his hammers are gone a chest drops in on the far side; it holds a Starman, and the map is
back without him (the junction becomes the checkpoint). Lose and he stays. His moves on the
map, the fight and the hammers were checked frame by frame against the real game in mGBA.
Graphics (the Bro, the hammer, the chest, his map sprite) are extracted from your ROM; the
two arenas (levels 0x4E / 0x4F) too: re-run `sma4-extract`. Not ported: the walking sound,
the puff when he leaves the map.
Debug: `--map --map-bro 4,3` puts him on the junction right of START; `--level 4e` starts
the arena.

Current state: World 1-1 is drawn natively (Bevy `TilemapChunk`) from the ROM's tiles and
palettes. The player is "Pip", an original placeholder character, with placeholder physics
(`crates/sma4-core/src/player.rs`, to be replaced by the reverse-engineered values).
`sma4-extract` builds levels by running the ROM's own object generators and graphics
loader in an emulator; porting those routines to Rust will remove the Unicorn dependency.
Graphics browser: PgUp/PgDn to switch sheets.
