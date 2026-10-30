//! Runs ROM routines in Unicorn on a minimal GBA memory map, so the extractor can use the
//! game's own level loaders. Used only at extraction time; the game never emulates.

use sma4_rom::{ROM_BASE, decompress};
use unicorn_engine::{Arch, Mode, Prot, RegisterARM, Unicorn};

/// Return address that ends a call.
const STOP: u64 = 0x0203_F000;
const STACK_TOP: u64 = 0x0300_7F00;
const MAX_INSNS: usize = 2_000_000;

pub struct Machine {
    uc: Unicorn<'static, ()>,
}

#[derive(Debug)]
pub struct EmuError(pub String);

impl std::fmt::Display for EmuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

type Result<T> = std::result::Result<T, EmuError>;

fn err(what: impl std::fmt::Debug) -> EmuError {
    EmuError(format!("{what:?}"))
}

impl Machine {
    pub fn new(rom: &[u8]) -> Result<Self> {
        let mut uc = Unicorn::new(Arch::ARM, Mode::THUMB).map_err(err)?;
        for (base, size) in [
            (0x0000_0000u64, 0x4000u64), // BIOS
            (0x0200_0000, 0x4_0000),     // EWRAM
            (0x0300_0000, 0x8000),       // IWRAM
            (0x0400_0000, 0x1000),       // IO
            (0x0500_0000, 0x1000),       // palette
            (0x0600_0000, 0x1_8000),     // VRAM
            (0x0700_0000, 0x1000),       // OAM
            (0x0E00_0000, 0x1_0000),     // SRAM
        ] {
            uc.mem_map(base, size, Prot::ALL).map_err(err)?;
        }
        let rom_size = (rom.len() as u64 + 0xFFF) & !0xFFF;
        uc.mem_map(ROM_BASE as u64, rom_size, Prot::ALL)
            .map_err(err)?;
        uc.mem_write(ROM_BASE as u64, rom).map_err(err)?;
        // Null entries in generator tables jump to 0 and slide to 0x08: ARM "movs pc, lr".
        uc.mem_write(0x08, &0xE1B0_F00Eu32.to_le_bytes())
            .map_err(err)?;
        uc.add_intr_hook(swi).map_err(err)?;
        Ok(Self { uc })
    }

    pub fn clear_ram(&mut self) -> Result<()> {
        self.uc
            .mem_write(0x0200_0000, &vec![0; 0x4_0000])
            .map_err(err)?;
        self.uc
            .mem_write(0x0300_0000, &vec![0; 0x8000])
            .map_err(err)
    }

    pub fn w8(&mut self, addr: u32, v: u8) -> Result<()> {
        self.uc.mem_write(addr as u64, &[v]).map_err(err)
    }

    pub fn w16(&mut self, addr: u32, v: u16) -> Result<()> {
        self.uc
            .mem_write(addr as u64, &v.to_le_bytes())
            .map_err(err)
    }

    pub fn w32(&mut self, addr: u32, v: u32) -> Result<()> {
        self.uc
            .mem_write(addr as u64, &v.to_le_bytes())
            .map_err(err)
    }

    pub fn write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        self.uc.mem_write(addr as u64, data).map_err(err)
    }

    pub fn r32(&self, addr: u32) -> Result<u32> {
        let b = self.read(addr, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read(&self, addr: u32, len: usize) -> Result<Vec<u8>> {
        self.uc.mem_read_as_vec(addr as u64, len).map_err(err)
    }

    /// Calls a Thumb function with up to four arguments and runs it to completion.
    pub fn call(&mut self, func: u32, args: &[u32]) -> Result<()> {
        let uc = &mut self.uc;
        // Mode first: SP is banked per mode, so a value set before the switch is lost.
        uc.reg_write(RegisterARM::CPSR, 0x3F).map_err(err)?; // system mode, Thumb
        uc.reg_write(RegisterARM::SP, STACK_TOP).map_err(err)?;
        uc.reg_write(RegisterARM::LR, STOP | 1).map_err(err)?;
        let regs = [
            RegisterARM::R0,
            RegisterARM::R1,
            RegisterARM::R2,
            RegisterARM::R3,
        ];
        for (r, &a) in regs.iter().zip(args) {
            uc.reg_write(*r, a as u64).map_err(err)?;
        }
        uc.emu_start((func | 1) as u64, STOP, 0, MAX_INSNS)
            .map_err(|e| EmuError(format!("{e:?} in {func:#010x}")))?;
        let pc = uc.reg_read(RegisterARM::PC).map_err(err)?;
        if pc & !1 != STOP {
            return Err(EmuError(format!(
                "{func:#010x} did not return (pc {pc:#x})"
            )));
        }
        Ok(())
    }
}

/// High-level BIOS. Unicorn has already stepped past the Thumb `svc #n`.
fn swi(uc: &mut Unicorn<'_, ()>, _intno: u32) {
    let reg = |uc: &Unicorn<'_, ()>, r| uc.reg_read(r).unwrap_or(0) as u32;
    let pc = reg(uc, RegisterARM::PC);
    let num = uc
        .mem_read_as_vec(pc as u64 - 2, 1)
        .map(|b| b[0])
        .unwrap_or(0xFF);
    let (r0, r1, r2) = (
        reg(uc, RegisterARM::R0),
        reg(uc, RegisterARM::R1),
        reg(uc, RegisterARM::R2),
    );
    match num {
        0x06 => {
            // Div: r0 / r1 -> r0 quotient, r1 remainder, r3 |quotient|
            let (a, b) = (r0 as i32, (r1 as i32).max(1));
            let q = a.wrapping_div(b);
            let _ = uc.reg_write(RegisterARM::R0, q as u32 as u64);
            let _ = uc.reg_write(RegisterARM::R1, a.wrapping_rem(b) as u32 as u64);
            let _ = uc.reg_write(RegisterARM::R3, q.unsigned_abs() as u64);
        }
        0x0B | 0x0C => {
            // CpuSet / CpuFastSet
            let count = (r2 & 0x1F_FFFF) as usize;
            let unit = if num == 0x0C || r2 & (1 << 26) != 0 {
                4
            } else {
                2
            };
            let data = if r2 & (1 << 24) != 0 {
                uc.mem_read_as_vec(r0 as u64, unit).map(|v| v.repeat(count))
            } else {
                uc.mem_read_as_vec(r0 as u64, unit * count)
            };
            if let Ok(d) = data {
                let _ = uc.mem_write(r1 as u64, &d);
            }
        }
        0x11 | 0x12 | 0x14 | 0x15 => {
            // LZ77 / RLE decompression (WRAM and VRAM variants behave the same here).
            let Ok(head) = uc.mem_read_as_vec(r0 as u64, 4) else {
                return;
            };
            let size = u32::from_le_bytes([head[1], head[2], head[3], 0]) as usize;
            // Worst-case packed size, clamped to the end of the source memory region.
            let region_end: u32 = match r0 >> 24 {
                0x08 => ROM_BASE + 0x80_0000,
                0x02 => 0x0204_0000,
                0x03 => 0x0300_8000,
                _ => r0 + 4,
            };
            let len = (size * 2 + 0x400).min(region_end.saturating_sub(r0) as usize);
            let Ok(src) = uc.mem_read_as_vec(r0 as u64, len) else {
                return;
            };
            let out = if num < 0x13 {
                decompress::lz77(&src)
            } else {
                decompress::rle(&src)
            };
            if let Ok(out) = out {
                let _ = uc.mem_write(r1 as u64, &out);
            }
        }
        _ => eprintln!("warning: unhandled SWI {num:#x} at {pc:#x}"),
    }
}
