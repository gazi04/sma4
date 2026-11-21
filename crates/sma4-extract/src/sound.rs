//! Sound effects, rendered by the ROM's own sound driver (`sma4_assets::sound`).
//!
//! * Init 0x080E7908 (-> 0x080E8204 with 0x083084BC): SOUNDCNT 0xFF77 / 0x9A0D (Direct
//!   Sound A left, B right, both full volume), timer 0 reload 0xF9C4, PCM double buffers
//!   0x03000118 [A0, A1, B0, B1] (0xB0 bytes each, 176 samples a frame).
//! * Start 0x080E7D54(id, position): entry 0x08308564[id] (low 16 bits the data, bits 16-23
//!   the slot; with bit 24 a pan from the position's screen x, 0x080E7C80, and a volume,
//!   0x080E7CDC: no position (0) is the middle, full volume) queues commands at 0x03000398;
//!   0x080EA274 commits them (0x0300039C) for the next frame.
//! * Frame 0x080E82DC: the commands (0x080EA794), the sequencer (0x080E9F4C), the PSG
//!   channels (0x080E9070) and the PCM mix (0x080E8E5C) into the buffers of parity
//!   0x0300012A; the VBlank (0x080E82D0 -> 0x080E8394) points DMA 1 / 2 at them for FIFO A
//!   / B and flips the parity.
//!
//! The effects the port plays use only the PCM channels (no PSG register changes while
//! they play), so the two buffers are the whole sound.

use sma4_assets::sound::{SOUND_IDS, SoundAsset};
use sma4_rom::Rom;

use crate::emu::Machine;
use crate::level::ExtractError;

const FN_SOUND_INIT: u32 = 0x080E_7908;
const FN_SOUND_START: u32 = 0x080E_7D54;
const FN_SOUND_FRAME: u32 = 0x080E_82DC;
const FN_SOUND_COMMIT: u32 = 0x080E_A274;
const PCM_PARITY: u32 = 0x0300_012A;
const PCM_BUFFERS: u32 = 0x0300_0118;
const FRAME_SAMPLES: usize = 0xB0;
const SOUND_TABLE: u32 = 0x0830_8564;
/// The PSG registers (SOUND1CNT_L .. the wave RAM's start): left as the init set them.
const PSG_REGS: u32 = 0x0400_0060;
/// Long enough for any effect; rendering stops after the last loud frame.
const MAX_FRAMES: usize = 600;

pub fn extract_sounds(rom: &Rom, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    for &id in SOUND_IDS {
        let sound = render(rom, id)?;
        sound.write(out_dir, id).map_err(|e| ExtractError(e.to_string()))?;
    }
    println!("  sound effects: {}", SOUND_IDS.len());
    Ok(())
}

fn render(rom: &Rom, id: u8) -> Result<SoundAsset, ExtractError> {
    let mut m = Machine::new(rom.bytes()).map_err(|e| ExtractError(e.to_string()))?;
    m.call(FN_SOUND_INIT, &[])?;
    m.call(FN_SOUND_START, &[id as u32, 0])?;
    m.call(FN_SOUND_COMMIT, &[])?;
    let psg = m.read(PSG_REGS, 0x20)?;
    let mut samples = Vec::new();
    let mut last = 0;
    for f in 0..MAX_FRAMES {
        m.call(FN_SOUND_FRAME, &[])?;
        let p = m.read(PCM_PARITY, 1)?[0] as u32;
        let a = m.read(m.r32(PCM_BUFFERS + p * 4)?, FRAME_SAMPLES)?;
        let b = m.read(m.r32(PCM_BUFFERS + 8 + p * 4)?, FRAME_SAMPLES)?;
        if m.read(PSG_REGS, 0x20)? != psg {
            return Err(ExtractError(format!("sound {id:#x} uses the PSG channels (not rendered)")));
        }
        for (l, r) in a.iter().zip(&b) {
            samples.extend([*l as i8, *r as i8]);
        }
        if a.iter().chain(&b).any(|&x| x != 0) {
            last = f + 1;
        }
        m.w8(PCM_PARITY, 1 - p as u8)?;
    }
    samples.truncate(last * FRAME_SAMPLES * 2);
    let slot = (rom.u32(SOUND_TABLE + id as u32 * 4)? >> 16) as u8;
    Ok(SoundAsset { slot, samples })
}
