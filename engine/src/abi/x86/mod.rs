mod exit;
mod state;

pub use exit::{
    ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EXIT_SIZE, FAULT_ADDRESS_OFFSET,
    REASON_OFFSET, RETIRED_OFFSET, decode_exit, encode_exit,
};

pub use state::{
    EFLAGS_OFFSET, EIP_OFFSET, REGISTERS_OFFSET, STATE_SIZE, decode_state, encode_state,
};
