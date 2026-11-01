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
