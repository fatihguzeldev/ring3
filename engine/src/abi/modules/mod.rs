#![forbid(unsafe_code)]

mod gates;
mod operation;

pub use gates::{
    MODULE_GATE_PAGE_HEADER_SIZE, MODULE_GATES_HEADER_SIZE, encode_module_gate_page,
    encode_module_gates, module_gates_size,
};
pub use operation::{MODULE_RECORD_SIZE, ModuleRecord32, decode_module, encode_module};

use super::{AbiError, header};
#[cfg(test)]
use crate::cpu::dbt::GateSpec;

#[cfg(test)]
mod tests;
