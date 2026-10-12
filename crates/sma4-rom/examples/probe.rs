//! Prints header info and the LZ77 streams found in a ROM.
//! Usage: cargo run -p sma4-rom --example probe -- path/to/rom.gba

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe <rom.gba>")?;
    let rom = sma4_rom::Rom::open(path)?;
    println!("{:?}", rom.header());
    let found = rom.scan_lz77(256, 32);
    let total: usize = found.iter().map(|f| f.unpacked_len).sum();
    println!("{} LZ77 streams, {} KiB unpacked", found.len(), total / 1024);
    for f in found.iter().take(15) {
        println!("  {:#010x} packed {:6} -> {:6}", f.addr, f.packed_len, f.unpacked_len);
    }
    Ok(())
}
