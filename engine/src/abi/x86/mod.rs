mod exit;
mod segments;
mod simd;
mod state;
mod x87;

pub use exit::{
    ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EXIT_SIZE, EXIT_VERSION_2, EXIT_VERSION_3,
    EXIT_VERSION_4, EXIT_VERSION_5, EXIT_VERSION_6, EXIT_VERSION_7, FAULT_ADDRESS_OFFSET,
    REASON_OFFSET, RETIRED_OFFSET, decode_exit, encode_exit, encode_exit_v2, encode_exit_v3,
    encode_exit_v4, encode_exit_v5, encode_exit_v6, encode_exit_v7,
};

pub use segments::{
    SEGMENTS_BASES_OFFSET, SEGMENTS_SELECTORS_OFFSET, SEGMENTS_SIZE, decode_segments,
    encode_segments,
};
pub use simd::{
    MXCSR_MASK, SIMD_MXCSR_OFFSET, SIMD_SIZE, SIMD_XMM_OFFSET, decode_simd, encode_simd,
};
pub use state::{
    EFLAGS_OFFSET, EIP_OFFSET, LEGACY_FLAGS_MASK, REGISTERS_OFFSET, STATE_METADATA_OFFSET,
    STATE_SIZE, STATE_VERSION_2, USER_FLAGS_MASK, decode_state, encode_state, encode_state_v2,
};
pub(crate) use state::{StateMetadata, decode_state_metadata, encode_state_current};

pub use x87::{
    X87_CODE_SELECTOR_OFFSET, X87_CONTROL_OFFSET, X87_DATA_POINTER_OFFSET,
    X87_DATA_SELECTOR_OFFSET, X87_INSTRUCTION_POINTER_OFFSET, X87_OPCODE_OFFSET,
    X87_REGISTERS_OFFSET, X87_SIZE, X87_STATUS_OFFSET, X87_TAG_OFFSET, decode_x87, encode_x87,
};
