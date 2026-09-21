pub(super) struct Sequence(u32);

impl Default for Sequence {
    fn default() -> Self {
        Self(1)
    }
}

impl Sequence {
    pub(super) fn seed(&mut self, seed: u32) {
        self.0 = seed;
    }

    pub(super) fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 16) & 0x7fff
    }
}
