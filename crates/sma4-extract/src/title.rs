//! The title screen the game-over menu's QUIT goes back to (`screens/title.bin`, see
//! `sma4_assets::TitleAsset`): VRAM after the ROM's own graphics load, the ROM tables and
//! maps the title's logic reads, and Mario's and Luigi's tiles for every pose as the
//! player DMA leaves them.

use sma4_assets::{TITLE_FORMS, TITLE_LZ, TITLE_PLAYER_BLOCK, TITLE_POSES, TITLE_ROM, TitleAsset, title_player_block};
use sma4_rom::Rom;

use crate::emu::Machine;
use crate::level::ExtractError;

/// The title's state block pointer (0x0802AC70 takes it from the heap).
const STATE_PTR: u32 = 0x0300_7828;
const STATE_BUF: u32 = 0x0203_D000;
const LANGUAGE: u32 = 0x0300_3777;
/// BG0-3 control shadows as 0x0802BE9C sets them: the graphics load reads their char and
/// screen bases.
const BG_CNT: u32 = 0x0300_2DA4;
const BG_CNT_TITLE: [u16; 4] = [0xD400, 0xD801, 0xDC02, 0x0F03];
const VRAM: u32 = 0x0600_0000;
const VRAM_LEN: usize = 0x1_8000;

const FN_GFX: u32 = 0x0802_AE30;
/// 0x08035AB0(player): pose pointers from +0x11 (pose) and +0x12 (form); 0x08035B88: the
/// DMA of both players' tiles (the other player's pointers, left 0, copy nothing).
const FN_POSE: u32 = 0x0803_5AB0;
const FN_DMA: u32 = 0x0803_5B88;
const PLAYER_SIZE: u32 = 0x30;
const POSE: u32 = 0x11;
const FORM: u32 = 0x12;
/// The DMA's rows: 0x06011000 + row * 0x400, 0x80 bytes a player.
const DMA_ROWS: u32 = 0x0601_1000;
const DMA_ROW_STEP: u32 = 0x400;
const DMA_PLAYER_W: u32 = 0x80;

pub fn extract(rom: &Rom, m: &mut Machine, out: &std::path::Path) -> Result<(), ExtractError> {
    m.clear_ram()?;
    m.w32(STATE_PTR, STATE_BUF)?;
    m.w8(LANGUAGE, 0)?;
    for (i, &cnt) in BG_CNT_TITLE.iter().enumerate() {
        m.w16(BG_CNT + i as u32 * 2, cnt)?;
    }
    m.call(FN_GFX, &[])?;
    let vram = m.read(VRAM, VRAM_LEN)?;

    let mut players = vec![Vec::new(); 2 * TITLE_FORMS * TITLE_POSES];
    for player in 0..2u32 {
        for form in 0..TITLE_FORMS {
            for pose in 0..TITLE_POSES {
                m.write(STATE_BUF, &[0; 0x100])?;
                let base = STATE_BUF + player * PLAYER_SIZE;
                m.w8(base + POSE, pose as u8)?;
                m.w8(base + FORM, form as u8)?;
                m.call(FN_POSE, &[player])?;
                m.call(FN_DMA, &[])?;
                let mut block = Vec::with_capacity(TITLE_PLAYER_BLOCK);
                for row in 0..4 {
                    let at = DMA_ROWS + row * DMA_ROW_STEP;
                    let own = m.read(at + player * DMA_PLAYER_W, DMA_PLAYER_W as usize)?;
                    let other = m.read(at + (player ^ 1) * DMA_PLAYER_W, DMA_PLAYER_W as usize)?;
                    if other.iter().any(|&b| b != 0) {
                        return Err(ExtractError(format!(
                            "title player {player} pose {pose:#x} spills into the other player's tiles"
                        )));
                    }
                    block.extend_from_slice(&own);
                }
                players[title_player_block(player as usize, form, pose)] = block;
            }
        }
    }

    let mut asset = TitleAsset { vram, players, ..Default::default() };
    for &(start, end) in TITLE_ROM {
        asset.rom.push((start, rom.slice(start, (end - start) as usize)?.to_vec()));
    }
    for &addr in TITLE_LZ {
        asset.lz.push((addr, rom.decompress(addr)?));
    }
    asset.write(out).map_err(|e| ExtractError(e.to_string()))?;
    println!("  title screen");
    Ok(())
}
