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

Current state: World 1-1 is drawn natively (Bevy `TilemapChunk`) from the ROM's tiles and
palettes. The player is "Pip", an original placeholder character, with placeholder physics
(`crates/sma4-core/src/player.rs`, to be replaced by the reverse-engineered values).
`sma4-extract` builds levels by running the ROM's own object generators and graphics
loader in an emulator; porting those routines to Rust will remove the Unicorn dependency.
Graphics browser: PgUp/PgDn to switch sheets.
