mod command;
mod encoder;
mod packet;
mod pipeline;
mod readback;
mod shader;

pub use command::{append_command, append_payload, payload_size};
pub use packet::{
    BatchHeader, Fragment, FragmentIter, append_fragment, packet, write_batch_header,
};
pub use readback::HEADER_SIZE as READBACK_HEADER_SIZE;
pub use readback::{ReadbackReceipt, ReadbackStatus, encode_readback_receipt, readback_receipt};
pub use readback::{ReadbackHeader, readback_header};

pub const BATCH_HEADER_SIZE: usize = 64;
pub const FRAGMENT_HEADER_SIZE: usize = 32;
pub const VERSION: u16 = 2;
pub const PROFILE: u16 = 8;
pub const FIRST_FRAGMENT: u16 = 1;
pub const LAST_FRAGMENT: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    Length,
    Magic,
    Version,
    Reserved,
    InvalidCommand,
    Identity,
    Allocation,
}

#[cfg(test)]
mod tests;
