use crate::{RomError, Result};

/// Regional builds. ROM offsets differ between them, so data tables are keyed on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    /// AX4E, North America.
    Usa { revision: u8 },
    /// AX4P, Europe.
    Europe { revision: u8 },
    /// AX4J, Japan.
    Japan { revision: u8 },
}

#[derive(Debug, Clone)]
pub struct Header {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub version: Version,
}

impl Header {
    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x200 {
            return Err(RomError::TooSmall(data.len()));
        }
        let text = |r: std::ops::Range<usize>| {
            String::from_utf8_lossy(&data[r]).trim_end_matches('\0').to_string()
        };
        let title = text(0xA0..0xAC);
        let game_code = text(0xAC..0xB0);
        let maker_code = text(0xB0..0xB2);
        let revision = data[0xBC];

        // Complement check over 0xA0..=0xBC, as the BIOS does at boot.
        let computed = data[0xA0..=0xBC]
            .iter()
            .fold(0u8, |c, &b| c.wrapping_sub(b))
            .wrapping_sub(0x19);
        let stored = data[0xBD];
        if computed != stored {
            return Err(RomError::BadChecksum { stored, computed });
        }

        let version = match game_code.as_str() {
            "AX4E" => Version::Usa { revision },
            "AX4P" => Version::Europe { revision },
            "AX4J" => Version::Japan { revision },
            _ => return Err(RomError::WrongGame(game_code)),
        };
        Ok(Self { title, game_code, maker_code, version })
    }
}
