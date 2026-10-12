/// Button set using the REG_KEYINPUT bit layout (active-high here; the register is
/// active-low, see [`Buttons::from_keyinput`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Buttons(u16);

impl Buttons {
    pub const A: Self = Self(1 << 0);
    pub const B: Self = Self(1 << 1);
    pub const SELECT: Self = Self(1 << 2);
    pub const START: Self = Self(1 << 3);
    pub const RIGHT: Self = Self(1 << 4);
    pub const LEFT: Self = Self(1 << 5);
    pub const UP: Self = Self(1 << 6);
    pub const DOWN: Self = Self(1 << 7);
    pub const R: Self = Self(1 << 8);
    pub const L: Self = Self(1 << 9);

    const MASK: u16 = 0x3FF;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & Self::MASK)
    }

    /// Converts a raw REG_KEYINPUT value (0 = pressed).
    pub const fn from_keyinput(reg: u16) -> Self {
        Self(!reg & Self::MASK)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Buttons {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Buttons {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for Buttons {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::Not for Buttons {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0 & Self::MASK)
    }
}
