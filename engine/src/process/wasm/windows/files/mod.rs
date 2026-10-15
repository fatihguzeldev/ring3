mod configuration;
mod environment;
mod mounting;
mod overlay;
mod progress;
mod transport;
mod uploads;
pub(crate) use configuration::*;
pub(crate) use environment::*;
pub(crate) use mounting::*;
pub(crate) use overlay::*;
pub(crate) use progress::*;
use transport::{status, storage_guard, storage_input, storage_output, storage_status};
use uploads::input_status;
pub(crate) use uploads::*;

use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    process::{EngineInstance, HostError},
};
fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}
