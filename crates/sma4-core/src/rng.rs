//! The ROM's random generator: 19 bytes at 0x03003878 (seeded 0x88, 0... by 0x08094CBC /
//! 0x080ADF48), shifted once per main-loop frame (0x08000698 -> 0x08026294). Objects read
//! the byte 0x03003879[slot]. Checked against the ROM routine for 5000 steps.

/// 0x08026294's register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng([u8; 19]);

impl Default for Rng {
    fn default() -> Self {
        let mut b = [0; 19];
        b[0] = 0x88;
        Self(b)
    }
}

impl Rng {
    /// 0x08026294: the new top bit is bit 1 of byte 0 xor bit 1 of byte 1; every byte shifts
    /// right, its low bit going into the next byte's top.
    pub fn step(&mut self) {
        let mut carry = (self.0[1] & 2 ^ self.0[0] & 2) != 0;
        for b in &mut self.0 {
            let out = *b & 1 != 0;
            *b = (carry as u8) << 7 | *b >> 1;
            carry = out;
        }
    }

    /// Sets byte 0 (the title, 0x0802C044, writes 0x88 there).
    pub fn set_first(&mut self, b: u8) {
        self.0[0] = b;
    }

    /// 0x03003879[slot] (slots wrap at 18).
    pub fn byte(&self, slot: usize) -> u8 {
        self.0[1 + slot % 18]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_the_feedback_bit_in_at_the_top() {
        let mut r = Rng::default();
        r.step();
        // Byte 0 0x88: bit 1 clear, byte 1 0: feedback 0; 0x88 >> 1 = 0x44.
        assert_eq!(r.0[0], 0x44);
        for _ in 0..200 {
            r.step();
        }
        assert!(r.0.iter().any(|&b| b != 0), "never dies out");
    }
}
