//! Doors (0x0809588C, the player's collision; see docs/re-notes.md, "Doors"):
//!
//! * The body probe (0x08099FB8 at the frame + (8, 0x14), or + (8, 0x0A) for a big player
//!   who is not crouching) reads the door code 0x43: a small player must be at the main-plane
//!   door bottom (0x043), a big one at the top, a second-plane tile mapped to 0x43
//!   (0x0813E174[tileset], 0x100 in the fortress). In tilesets of class 1 the code 0x42
//!   (the fortress's dark door, 0x102 / 0x103) is a door at any height.
//! * Up pressed (0x03003AEC bit 6) standing on the ground, carrying nothing: the area switch
//!   (0x03003CA0 = 1), by the junction pointer of the page the player is on. A wooden door
//!   (0x43) centres the frame on its column first (x | 8 rounds up, then & ~0xF), sound 0x62.
//! * Arrival (pointer direction 8): the player stands at the pointer's place.
//! * In the crushing-ceiling room, doors only open with the ceiling up (`Crusher::door_open`).

use crate::Buttons;
use crate::level::{Level, TILE_PX};
use crate::player::{Form, Player};

/// Door codes.
const DOOR: u8 = 0x43;
const DARK_DOOR: u8 = 0x42;
/// Tileset class where 0x42 is a door (the fortress's).
pub const DARK_DOOR_CLASS: u8 = 1;
/// Body probe below the frame's top.
const BODY_SMALL: i32 = 0x14;
const BODY_BIG: i32 = 0x0A;
/// Exit direction of a door pointer.
pub const DOOR_EXIT: u8 = 8;

/// The door the player can go through: Some(frame x to snap to) for a wooden door, Some(None)
/// for a dark one.
pub fn entry(level: &Level, p: &Player, pressed: Buttons, class: u8) -> Option<Option<i32>> {
    if !pressed.contains(Buttons::UP) || !p.on_ground {
        return None;
    }
    let (fx, fy) = p.frame();
    let big = p.form != Form::Small;
    let y = fy + if big { BODY_BIG } else { BODY_SMALL };
    let (tx, ty) = ((fx + 8).div_euclid(TILE_PX), level.row_at(y));
    let m = level.metatile(tx, ty)?;
    let code = level.collision_id(tx, ty)?;
    if code == DARK_DOOR && class == DARK_DOOR_CLASS {
        return Some(None);
    }
    // Small: the main-plane door tile; big: the second-plane one (+0x40 of the probe).
    if code != DOOR || (m >= 0x100) != big {
        return None;
    }
    let x = fx + if fx & 8 != 0 { 0x10 } else { 0 };
    Some(Some(x & !0xF))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{SKY, TEST_SOLIDITY};
    use sma4_assets::BlockRules;

    #[test]
    fn up_at_a_door_enters_it_centred() {
        let (w, h) = (8, 6);
        let mut m = vec![SKY; w * h];
        for x in 0..w {
            m[5 * w + x] = 0x53;
        }
        m[3 * w + 3] = 0x100;
        m[4 * w + 3] = DOOR as u16;
        let mut level = Level::new(w, h, m, TEST_SOLIDITY, BlockRules::default());
        level.solidity.high[0] = DOOR;
        let mut p = Player::spawn(3 * 16 + 6, 4 * 16);
        p.on_ground = true;
        assert_eq!(p.form, Form::Small);
        assert_eq!(entry(&level, &p, Buttons::UP, 0), Some(Some(0x30)));
        assert_eq!(entry(&level, &p, Buttons::empty(), 0), None);
    }
}
