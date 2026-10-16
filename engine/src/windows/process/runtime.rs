use crate::cpu::x86::capabilities::{CPU_ID_LEAVES, NX_ENABLED};

pub(crate) struct PointerCodec32 {
    cookie: u32,
}

impl PointerCodec32 {
    pub(crate) fn new(instance_key: u64) -> Self {
        let mixed = instance_key ^ instance_key.rotate_left(29);
        Self {
            cookie: (mixed as u32 ^ (mixed >> 32) as u32).wrapping_mul(0x9e37_79b1),
        }
    }

    pub(crate) fn encode(&self, pointer: u32) -> u32 {
        (pointer ^ self.cookie).rotate_right(self.cookie & 31)
    }

    pub(crate) fn decode(&self, pointer: u32) -> u32 {
        pointer.rotate_left(self.cookie & 31) ^ self.cookie
    }
}

pub(crate) fn processor_feature_present(feature: u32) -> bool {
    let (function, register, bit) = match feature {
        2 => (1, 3, 8),
        3 => (1, 3, 23),
        6 => (1, 3, 25),
        8 => (1, 3, 26),
        9 => (1, 3, 6),
        10 => return NX_ENABLED,
        12 => (1, 2, 0),
        13 => (1, 3, 8),
        17 => (1, 2, 26),
        36 => (1, 2, 9),
        37 => (1, 2, 19),
        38 => (1, 2, 20),
        39 => (1, 2, 28),
        40 => (7, 1, 5),
        41 => (7, 1, 16),
        _ => return false,
    };
    CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == function && leaf.subfunction.unwrap_or(0) == 0)
        .is_some_and(|leaf| leaf.registers[register] & (1 << bit) != 0)
}
