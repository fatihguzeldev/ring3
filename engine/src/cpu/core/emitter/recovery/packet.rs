//! cpu-private transient recovery transport; no architectural fp authority lives here.

pub(crate) const MAGIC: u32 = 0x5233_504b;
pub(crate) const VERSION: u32 = 1;
pub(crate) const MAX_CAPTURE_COLORS: u32 = 8192;
pub(crate) const CELL_BYTES: u32 = 16;
pub(crate) const HEADER_BYTES: u32 = 1024;
pub(crate) const CELLS_OFFSET: u32 = HEADER_BYTES;
pub(crate) const WORKSPACE_BYTES: u32 = HEADER_BYTES + MAX_CAPTURE_COLORS * CELL_BYTES;
pub(crate) const OUTPUT_CAPACITY: usize = 64;

pub(crate) const MAGIC_OFFSET: u32 = 0;
pub(crate) const VERSION_OFFSET: u32 = 4;
pub(crate) const BYTES_OFFSET: u32 = 8;
pub(crate) const CAPACITY_OFFSET: u32 = 12;
pub(crate) const CONTEXT: u32 = 16;
pub(crate) const CONTINUATION: u32 = 20;
pub(crate) const MEMORY: u32 = 24;
pub(crate) const ARMED_NONCE: u32 = 32;
pub(crate) const CELLS: u32 = 40;
pub(crate) const STATE: u32 = 48;
pub(crate) const EPOCH: u32 = 52;
pub(crate) const EXPECTED_OWNER: u32 = 56;
pub(crate) const EXPECTED_SLOT: u32 = 64;
pub(crate) const COLORS: u32 = 68;
pub(crate) const EXPECTED_GENERATION: u32 = 72;
pub(crate) const EXPECTED_NONCE: u32 = 80;
pub(crate) const BLOB: u32 = 88;
pub(crate) const SIGNED_SITE: u32 = 92;
pub(crate) const RECIPES_BASE: u32 = 96;
pub(crate) const RECIPES_COUNT: u32 = 100;
pub(crate) const SITE_WORDS: u32 = 104;
pub(crate) const EXPRESSION_BASE: u32 = 108;
pub(crate) const FP_ACTIVE: u32 = 112;
pub(crate) const FP_DIRTY: u32 = 116;
pub(crate) const SELECTED: u32 = 120;
pub(crate) const EXACT_SUFFIX: u32 = 124;
pub(crate) const BANK: u32 = 128;
pub(crate) const POOL: u32 = 132;
pub(crate) const PREFIX_BASE: u32 = 136;
pub(crate) const REMAIN: u32 = 140;
pub(crate) const RETIRED: u32 = 144;
pub(crate) const EIP: u32 = 148;
pub(crate) const OUTPUT_MASK: u32 = 152;
pub(crate) const OUTPUTS: u32 = 160;
pub(crate) const VM_PRIVATE: u32 = 672;
pub(crate) const CELL_BITS: u32 = 0;
pub(crate) const CELL_TAG: u32 = 8;
pub(crate) const CELL_EPOCH: u32 = 12;

pub(crate) const IDLE: u32 = 0;
pub(crate) const PHASE_ZERO: u32 = 1;
pub(crate) const PHASE_ONE: u32 = 2;
pub(crate) const READY: u32 = 3;
pub(crate) const DONE: i32 = 0;
pub(crate) const CAPTURE_PHASE_ZERO: i32 = 1;
pub(crate) const CAPTURE_PHASE_ONE: i32 = 2;
pub(crate) const REJECT: i32 = 3;

// the roles are finite architectural destinations, never arbitrary caller locals.
pub(crate) const OUTPUT_LOCALS: [u32; 59] = [
    2, 3, 4, 5, 6, 7, 8, 9, 11, 20, 25, 21, 22, 23, 24, 87, 88, 90, 91, 92, 93, 94, 95, 100, 101,
    102, 103, 76, 77, 78, 79, 80, 81, 82, 83, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
    120, 121, 122, 123, 124, 125, 85, 109, 158, 177, 10, 13, 12, 131,
];
pub(crate) const OUTPUT_VALID_MASK: u64 = (1_u64 << OUTPUT_LOCALS.len()) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputType {
    I32,
    I64,
    F64,
}

pub(crate) const fn output_type(local: u32) -> OutputType {
    if (local >= 76 && local < 84) || (local >= 101 && local < 104) {
        OutputType::F64
    } else if (local >= 110 && local < 118) || local == 177 {
        OutputType::I64
    } else {
        OutputType::I32
    }
}

pub(crate) fn output_role(local: u32) -> Option<u32> {
    OUTPUT_LOCALS
        .iter()
        .position(|&value| value == local)
        .map(|value| value as u32)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C, align(8))]
pub(crate) struct Cell {
    pub bits: u64,
    pub tag: i32,
    pub epoch: u32,
}

#[derive(Debug)]
#[repr(C, align(8))]
pub(crate) struct Packet {
    pub magic: u32,
    pub version: u32,
    pub bytes: u32,
    pub capacity: u32,
    pub context: u32,
    pub continuation: u32,
    pub memory: u64,
    pub armed_nonce: u64,
    pub cells_offset: u32,
    pub reserved: u32,
    pub state: u32,
    pub epoch: u32,
    pub expected_owner: u64,
    pub expected_slot: u32,
    pub colors: u32,
    pub expected_generation: u64,
    pub expected_nonce: u64,
    pub blob: u32,
    pub signed_site: i32,
    pub recipes_base: u32,
    pub recipes_count: u32,
    pub site_words: u32,
    pub expression_base: u32,
    pub fp_active: u32,
    pub fp_dirty: u32,
    pub selected: i32,
    pub exact_suffix: u32,
    pub bank: u32,
    pub pool: u32,
    pub prefix_base: u32,
    pub remain: u32,
    pub retired: u32,
    pub eip: u32,
    pub output_mask: u64,
    pub outputs: [u64; OUTPUT_CAPACITY],
    pub vm_private: [u64; 44],
}

impl Default for Packet {
    fn default() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            bytes: WORKSPACE_BYTES,
            capacity: MAX_CAPTURE_COLORS,
            context: 0,
            continuation: 0,
            memory: 0,
            armed_nonce: 0,
            cells_offset: CELLS_OFFSET,
            reserved: 0,
            state: IDLE,
            epoch: 0,
            expected_owner: 0,
            expected_slot: 0,
            colors: 0,
            expected_generation: 0,
            expected_nonce: 0,
            blob: 0,
            signed_site: 0,
            recipes_base: 0,
            recipes_count: 0,
            site_words: 0,
            expression_base: 0,
            fp_active: 0,
            fp_dirty: 0,
            selected: 0,
            exact_suffix: 0,
            bank: 0,
            pool: 0,
            prefix_base: 0,
            remain: 0,
            retired: 0,
            eip: 0,
            output_mask: 0,
            outputs: [0; OUTPUT_CAPACITY],
            vm_private: [0; 44],
        }
    }
}

// ordinary lease cleanup invalidates the request, not the per-cell epoch namespace.
impl Packet {
    pub(crate) fn disarm(&mut self) {
        self.state = IDLE;
        self.armed_nonce = 0;
        self.output_mask = 0;
    }
}

const _: () = {
    assert!(std::mem::size_of::<Cell>() == CELL_BYTES as usize);
    assert!(std::mem::offset_of!(Cell, bits) == CELL_BITS as usize);
    assert!(std::mem::offset_of!(Cell, tag) == CELL_TAG as usize);
    assert!(std::mem::offset_of!(Cell, epoch) == CELL_EPOCH as usize);
    assert!(std::mem::size_of::<Packet>() == HEADER_BYTES as usize);
    assert!(std::mem::align_of::<Packet>() == 8);
    assert!(WORKSPACE_BYTES == 132096);
    assert!(std::mem::offset_of!(Packet, context) == CONTEXT as usize);
    assert!(std::mem::offset_of!(Packet, armed_nonce) == ARMED_NONCE as usize);
    assert!(std::mem::offset_of!(Packet, state) == STATE as usize);
    assert!(std::mem::offset_of!(Packet, epoch) == EPOCH as usize);
    assert!(std::mem::offset_of!(Packet, expected_owner) == EXPECTED_OWNER as usize);
    assert!(std::mem::offset_of!(Packet, expected_generation) == EXPECTED_GENERATION as usize);
    assert!(std::mem::offset_of!(Packet, outputs) == OUTPUTS as usize);
    assert!(std::mem::offset_of!(Packet, vm_private) == VM_PRIVATE as usize);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_offsets_match_native_and_generated_transport() {
        macro_rules! field {
            ($name:ident, $offset:ident) => {
                assert_eq!(std::mem::offset_of!(Packet, $name), $offset as usize);
            };
        }
        field!(magic, MAGIC_OFFSET);
        field!(version, VERSION_OFFSET);
        field!(bytes, BYTES_OFFSET);
        field!(capacity, CAPACITY_OFFSET);
        field!(context, CONTEXT);
        field!(continuation, CONTINUATION);
        field!(memory, MEMORY);
        field!(armed_nonce, ARMED_NONCE);
        field!(cells_offset, CELLS);
        field!(state, STATE);
        field!(epoch, EPOCH);
        field!(expected_owner, EXPECTED_OWNER);
        field!(expected_slot, EXPECTED_SLOT);
        field!(colors, COLORS);
        field!(expected_generation, EXPECTED_GENERATION);
        field!(expected_nonce, EXPECTED_NONCE);
        field!(blob, BLOB);
        field!(signed_site, SIGNED_SITE);
        field!(recipes_base, RECIPES_BASE);
        field!(recipes_count, RECIPES_COUNT);
        field!(site_words, SITE_WORDS);
        field!(expression_base, EXPRESSION_BASE);
        field!(fp_active, FP_ACTIVE);
        field!(fp_dirty, FP_DIRTY);
        field!(selected, SELECTED);
        field!(exact_suffix, EXACT_SUFFIX);
        field!(bank, BANK);
        field!(pool, POOL);
        field!(prefix_base, PREFIX_BASE);
        field!(remain, REMAIN);
        field!(retired, RETIRED);
        field!(eip, EIP);
        field!(output_mask, OUTPUT_MASK);
        field!(outputs, OUTPUTS);
        field!(vm_private, VM_PRIVATE);
        assert_eq!(
            HEADER_BYTES + MAX_CAPTURE_COLORS * CELL_BYTES,
            WORKSPACE_BYTES
        );
    }

    #[test]
    fn disarm_preserves_epoch_namespace_and_transient_output_bits() {
        let mut packet = Packet {
            state: READY,
            epoch: u32::MAX,
            armed_nonce: 0x1234,
            output_mask: OUTPUT_VALID_MASK,
            ..Packet::default()
        };
        packet.outputs[0] = 0x7ff8_dead_beef_0001;
        packet.disarm();
        assert_eq!(packet.state, IDLE);
        assert_eq!(packet.armed_nonce, 0);
        assert_eq!(packet.output_mask, 0);
        assert_eq!(packet.epoch, u32::MAX);
        assert_eq!(packet.outputs[0], 0x7ff8_dead_beef_0001);
    }

    #[test]
    fn finite_outputs_include_every_legacy_target_without_aliasing_roles() {
        let mut seen = std::collections::BTreeSet::new();
        for (role, &local) in OUTPUT_LOCALS.iter().enumerate() {
            assert!(seen.insert(local));
            assert_eq!(output_role(local), Some(role as u32));
        }
        assert_eq!(output_type(101), OutputType::F64);
        assert_eq!(output_type(117), OutputType::I64);
        assert_eq!(output_type(177), OutputType::I64);
        assert_eq!(output_type(125), OutputType::I32);
        assert_eq!(output_role(84), None);
        assert_eq!(OUTPUT_VALID_MASK >> OUTPUT_LOCALS.len(), 0);
    }
}
