mod avx;
mod exit;
mod segments;
mod simd;
mod state;
mod system_registers;
mod x87;
mod ymm;

pub use exit::{
    ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EXIT_SIZE, EXIT_VERSION_2, EXIT_VERSION_3,
    EXIT_VERSION_4, EXIT_VERSION_5, EXIT_VERSION_6, EXIT_VERSION_7, EXIT_VERSION_8, EXIT_VERSION_9,
    EXIT_VERSION_10, EXIT_VERSION_11, EXIT_VERSION_12, EXIT_VERSION_13, EXIT_VERSION_14,
    FAULT_ADDRESS_OFFSET, REASON_OFFSET, RETIRED_OFFSET, decode_exit, encode_exit, encode_exit_v2,
    encode_exit_v3, encode_exit_v4, encode_exit_v5, encode_exit_v6, encode_exit_v7, encode_exit_v8,
    encode_exit_v9, encode_exit_v10, encode_exit_v11, encode_exit_v12, encode_exit_v13,
    encode_exit_v14,
};

pub use segments::{
    SEGMENT_BASES_RECORD_SIZE, SEGMENTS_BASES_OFFSET, SEGMENTS_SELECTORS_OFFSET, SEGMENTS_SIZE,
    SEGMENTS_VERSION_2, SEGMENTS_VERSION_3, decode_current_segment_context, decode_segment_context,
    decode_segments, encode_current_segment_context, encode_segment_context, encode_segments,
};
pub use simd::{
    MXCSR_MASK, SIMD_MXCSR_OFFSET, SIMD_SIZE, SIMD_XMM_OFFSET, decode_simd, encode_simd,
};
pub use state::{
    EFLAGS_OFFSET, EIP_OFFSET, LEGACY_FLAGS_MASK, REGISTERS_OFFSET, STATE_METADATA_OFFSET,
    STATE_SIZE, STATE_VERSION_2, STATE_VERSION_3, USER_FLAGS_MASK, USER_FLAGS_MASK_V3,
    decode_state, encode_state, encode_state_v2, encode_state_v3,
};
pub(crate) use state::{StateMetadata, decode_state_metadata, encode_state_current};

pub use x87::{
    X87_CODE_SELECTOR_OFFSET, X87_CONTROL_OFFSET, X87_DATA_POINTER_OFFSET,
    X87_DATA_SELECTOR_OFFSET, X87_INSTRUCTION_POINTER_OFFSET, X87_OPCODE_OFFSET,
    X87_REGISTERS_OFFSET, X87_SIZE, X87_STATUS_OFFSET, X87_TAG_OFFSET, decode_x87, encode_x87,
};

pub(crate) use segments::{SEGMENTS_CACHE_OFFSETS, decode_current_code_key};

pub use ymm::{YMM_REGISTERS_OFFSET, YMM_SIZE, decode_ymm, encode_ymm};

pub(crate) use avx::{
    AVX_ADDRESS_LOGICAL_OFFSET, AVX_ADDRESS_MAGIC, AVX_ADDRESS_SEGMENT_OFFSET, AVX_ADDRESS_SIZE,
    AVX_ADDRESS_TRANSFER, AVX_ADDRESS_VERSION, AVX_OUTPUT_SIZE, AVX_OUTPUT_TRANSFER,
    GATHER_ACCESS_OFFSET, GATHER_ADDRESS_BASE_OFFSET, GATHER_ADDRESS_MAGIC,
    GATHER_ADDRESS_SEGMENT_OFFSET, GATHER_ADDRESS_SIZE, GATHER_ADDRESS_TRANSFER,
    GATHER_ADDRESS_VERSION, GATHER_COMPLETED_OFFSET, GATHER_DESTINATION_OFFSET,
    GATHER_DETAIL_OFFSET, GATHER_FAULT_ADDRESS_OFFSET, GATHER_FAULT_LANE_OFFSET,
    GATHER_MASK_OFFSET, GATHER_NO_FAULT, GATHER_OUTPUT_SIZE, GATHER_OUTPUT_TRANSFER,
    GATHER_PAGING_RESULT_VERSION, GATHER_RESULT_MAGIC, GATHER_RESULT_SIZE, GATHER_RESULT_VERSION,
    GATHER_STATUS_OFFSET, GatherFaultKind, GatherFaultResult, GatherPagingGeometry, GatherResult,
    decode_avx_address, decode_gather_address, decode_gather_paging_result,
    encode_gather_paging_result, encode_gather_result,
};

pub use system_registers::{
    SYSTEM_REGISTER_BANK_SIZE, decode_system_register_bank, encode_system_register_bank,
};
mod event;
pub use event::{CPU_EVENT_SITE_SIZE, encode_cpu_event_site32};
