#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "entries/native/replacement.rs"]
mod process_entry_regions;

#[path = "lifecycle/disposal/inputs/native.rs"]
mod process_installed_resident_disposal_inputs;

#[path = "native/identity.rs"]
mod process_resident;

#[path = "admission/native/binary.rs"]
mod process_resident_binary;

#[path = "native/calls.rs"]
mod process_resident_call;

#[path = "admission/native/control.rs"]
mod process_resident_control;

#[path = "lifecycle/disposal/native.rs"]
mod process_resident_disposal;

#[path = "entries/native/resident.rs"]
mod process_resident_entries;

#[path = "admission/native/gates.rs"]
mod process_resident_gate;

#[path = "admission/native/indirect.rs"]
mod process_resident_indirect;

#[path = "lifecycle/native/installation.rs"]
mod process_resident_installation;

#[path = "admission/native/read.rs"]
mod process_resident_memory;

#[path = "lifecycle/native/retirement.rs"]
mod process_resident_retirement;

#[path = "admission/native/stack.rs"]
mod process_resident_stack;

#[path = "admission/native/store.rs"]
mod process_resident_store;

#[path = "admission/native/unary.rs"]
mod process_resident_unary;

#[path = "../../support/cpu/fetch/native.rs"]
mod support_fetch;

fn assert_error_contract(
    actual: Option<ring3_engine::process::HostError>,
    expected: ring3_engine::process::HostError,
) {
    use ring3_engine::{
        cpu::dbt::{CompileError, InstructionError, RegistryError},
        cpu::x86::decode::DecodeError,
        process::HostError,
    };
    let Some(actual) = actual else {
        panic!("expected compilation failure {expected:?}");
    };
    let (actual, expected) = match (actual, expected) {
        (HostError::Compile(actual), HostError::Compile(expected)) => (actual, expected),
        (
            HostError::Resident(RegistryError::Compile(actual)),
            HostError::Resident(RegistryError::Compile(expected)),
        ) => (actual, expected),
        (actual, expected) => {
            assert_eq!(actual, expected);
            return;
        }
    };
    match (actual, expected) {
        (
            CompileError::Instruction {
                pc: actual_pc,
                cause: InstructionError::Decode(actual),
            },
            CompileError::Instruction {
                pc,
                cause: InstructionError::Decode(expected @ DecodeError::MemoryFault { .. }),
            },
        ) => {
            assert_eq!(actual_pc, pc);
            support_fetch::assert_decode_contract(actual, expected);
        }
        (actual, expected) => assert_eq!(actual, expected),
    }
}
