//! Private generated/native continuation wire format. All indices use zero as
//! absent; owner indices are cache arena indices plus one.

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const HEADER_SIZE: u64 = 120;
pub(crate) const FORMAT: u64 = 0;
pub(crate) const ACTIVE: u64 = 4;
pub(crate) const CONTEXT: u64 = 8;
pub(crate) const ROW_MASK: u64 = 12;
pub(crate) const NONCE: u64 = 16;
pub(crate) const MEMORY: u64 = 24;
pub(crate) const ROWS: u64 = 32;
pub(crate) const OWNERS: u64 = 36;
pub(crate) const OWNER_LEN: u64 = 40;
pub(crate) const LINK_OWNERS: u64 = 44;
pub(crate) const SLOT_REGIONS: u64 = 48;
pub(crate) const SLOT_GENERATIONS: u64 = 52;
pub(crate) const SLOT_LEN: u64 = 56;
pub(crate) const TRACE_ARENA: u64 = 60;
pub(crate) const OLDEST: u64 = 64;
pub(crate) const NEWEST: u64 = 68;
pub(crate) const CURRENT_INDEX: u64 = 72;
pub(crate) const CURRENT_SLOT: u64 = 76;
pub(crate) const CURRENT_ID: u64 = 80;
pub(crate) const CURRENT_GENERATION: u64 = 88;
pub(crate) const CURRENT_ENTRY: u64 = 96;
pub(crate) const CURRENT_PORT: u64 = 100;
pub(crate) const CURRENT_NONCE: u64 = 104;
pub(crate) const FLAGS: u64 = 112;
pub(crate) const TRANSIENT: u32 = 1;
pub(crate) const COLD: u32 = 2;
pub(crate) const SELECTED_UNBOUND: u32 = 4;

pub(crate) const OWNER_SIZE: u64 = 40;
pub(crate) const OWNER_ID: u64 = 0;
pub(crate) const OWNER_GENERATION: u64 = 8;
pub(crate) const OWNER_VALIDATION: u64 = 16;
pub(crate) const OWNER_SLOT: u64 = 24;
pub(crate) const OWNER_OLDER: u64 = 28;
pub(crate) const OWNER_NEWER: u64 = 32;
pub(crate) const OWNER_FLAGS: u64 = 36;
pub(crate) const BOUND: u32 = 1;
pub(crate) const WINDOW: u32 = 2;

pub(crate) const ROW_SIZE: u64 = 80;
pub(crate) const ROW_PC: u64 = 0;
pub(crate) const ROW_STATE: u64 = 4;
pub(crate) const EMPTY: u32 = 0;
pub(crate) const PRESENT: u32 = 1;
pub(crate) const DELETED: u32 = 2;
pub(crate) const CANDIDATES: u64 = 8;
pub(crate) const CANDIDATE_SIZE: u64 = 24;
pub(crate) const CANDIDATE_ID: u64 = 0;
pub(crate) const CANDIDATE_INDEX: u64 = 8;
pub(crate) const CANDIDATE_PORT: u64 = 12;
pub(crate) const CANDIDATE_WORK: u64 = 16;
pub(crate) const CANDIDATE_FLAGS: u64 = 20;
pub(crate) const NON_HOT: u32 = 1;
pub(crate) const WINDOW_ROOT: u32 = 2;
pub(crate) const HASH_MULTIPLIER: u32 = 0x9e37_79b1;

#[derive(Debug)]
#[repr(C, align(8))]
pub(crate) struct Header {
    pub format: u32,
    pub active: u32,
    pub context: u32,
    pub row_mask: u32,
    pub nonce: u64,
    pub memory: u64,
    pub rows: u32,
    pub owners: u32,
    pub owner_len: u32,
    pub link_owners: u32,
    pub slot_regions: u32,
    pub slot_generations: u32,
    pub slot_len: u32,
    pub trace_arena: u32,
    pub oldest: u32,
    pub newest: u32,
    pub current_index: u32,
    pub current_slot: u32,
    pub current_id: u64,
    pub current_generation: u64,
    pub current_entry: u32,
    pub current_port: u32,
    pub current_nonce: u64,
    pub flags: u32,
    pub reserved: u32,
}

impl Default for Header {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            active: 0,
            context: 0,
            row_mask: 0,
            nonce: 0,
            memory: 0,
            rows: 0,
            owners: 0,
            owner_len: 0,
            link_owners: 0,
            slot_regions: 0,
            slot_generations: 0,
            slot_len: 0,
            trace_arena: 0,
            oldest: 0,
            newest: 0,
            current_index: 0,
            current_slot: 0,
            current_id: 0,
            current_generation: 0,
            current_entry: 0,
            current_port: u32::MAX,
            current_nonce: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(8))]
pub(crate) struct Owner {
    pub identity: u64,
    pub generation: u64,
    pub validation: u64,
    pub slot: u32,
    pub older: u32,
    pub newer: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(8))]
pub(crate) struct Candidate {
    pub identity: u64,
    pub index: u32,
    pub port: u32,
    pub work: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(8))]
pub(crate) struct Row {
    pub pc: u32,
    pub state: u32,
    pub candidates: [Candidate; 3],
}

pub(crate) fn hash(pc: u32) -> u32 {
    (pc ^ (pc >> 16)).wrapping_mul(HASH_MULTIPLIER)
}

const _: () = {
    assert!(std::mem::size_of::<Header>() == HEADER_SIZE as usize);
    assert!(std::mem::offset_of!(Header, flags) == FLAGS as usize);
    assert!(std::mem::offset_of!(Header, current_nonce) == CURRENT_NONCE as usize);
    assert!(std::mem::size_of::<Owner>() == OWNER_SIZE as usize);
    assert!(std::mem::offset_of!(Owner, flags) == OWNER_FLAGS as usize);
    assert!(std::mem::size_of::<Row>() == ROW_SIZE as usize);
    assert!(std::mem::offset_of!(Row, candidates) == CANDIDATES as usize);
    assert!(std::mem::size_of::<Candidate>() == CANDIDATE_SIZE as usize);
};
