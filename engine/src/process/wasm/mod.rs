#![forbid(unsafe_code)]

use std::cell::RefCell;

use super::{CallError, EngineInstance, HostError, StoreCompletion};
use crate::cpu::dbt::RegistryError;

mod clock;
pub(crate) use clock::{advance_cpu_clock, configure_cpu_clock, observe_cpu_clock};

mod media;
pub(crate) use media::*;

struct Registry {
    opened: bool,
    instance: Option<EngineInstance>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = const {
        RefCell::new(Registry { opened: false, instance: None })
    };
}

pub(crate) fn open(pages: u32, low: u32, high: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.opened {
            return 6;
        }
        let key = u64::from(low) | (u64::from(high) << 32);
        match EngineInstance::new(pages, key) {
            Ok(instance) => {
                registry.instance = Some(instance);
                registry.opened = true;
                0
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn close() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        registry.opened = true;
        if let Some(instance) = registry.instance.as_mut() {
            instance.close();
        }
        0
    })
}

pub(crate) fn arena_ptr() -> u32 {
    inspect(|instance| {
        if instance.is_open() {
            instance.arena_address() as u32
        } else {
            0
        }
    })
}

pub(crate) fn arena_len() -> u32 {
    inspect(|instance| {
        if instance.is_open() {
            instance.arena().len() as u32
        } else {
            0
        }
    })
}

pub(super) fn inspect(operation: impl FnOnce(&EngineInstance) -> u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 0;
        };
        registry.instance.as_ref().map_or(0, operation)
    })
}

pub(super) fn mutate(operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        operation(instance).map_or_else(status, |()| 0)
    })
}

pub(super) fn status(error: HostError) -> u32 {
    match error {
        HostError::Closed => 5,
        HostError::ProcessExited => 21,
        HostError::InvalidRequest => 7,
        HostError::InvalidArtifact => 3,
        HostError::CodeInvalidated => 4,
        HostError::Memory(_) => 8,
        HostError::Loader(error) => match error {
            crate::loader::LoadError::Malformed => 19,
            crate::loader::LoadError::Unsupported => 20,
            crate::loader::LoadError::Capacity | crate::loader::LoadError::Memory(_) => 8,
        },
        HostError::Module(error) => match error {
            crate::loader::ModuleError32::Pe(error) => status(HostError::Loader(error)),
            crate::loader::ModuleError32::Memory(_)
            | crate::loader::ModuleError32::Allocation
            | crate::loader::ModuleError32::Capacity => 8,
            crate::loader::ModuleError32::Busy => 12,
            crate::loader::ModuleError32::InvalidRequest => 7,
            crate::loader::ModuleError32::InvalidToken => 14,
            crate::loader::ModuleError32::InvalidReturn => 13,
            crate::loader::ModuleError32::InvalidName => 22,
            crate::loader::ModuleError32::DuplicateModule => 23,
            crate::loader::ModuleError32::DuplicateSymbol => 24,
            crate::loader::ModuleError32::MissingModule => 25,
            crate::loader::ModuleError32::MissingSymbol => 26,
            crate::loader::ModuleError32::InvalidForwarder => 27,
            crate::loader::ModuleError32::ForwarderCycle => 28,
            crate::loader::ModuleError32::InvalidSystemExport => 29,
            crate::loader::ModuleError32::InvalidModuleKind => 30,
            crate::loader::ModuleError32::AddressConflict => 31,
            crate::loader::ModuleError32::StalePlan => 32,
            crate::loader::ModuleError32::ModuleInUse => 33,
            crate::loader::ModuleError32::NotFound => 34,
            crate::loader::ModuleError32::InvalidHandle => 35,
            crate::loader::ModuleError32::ReferenceExhausted => 36,
            crate::loader::ModuleError32::InitializationFailed => 37,
        },
        HostError::Compile(_) => 10,
        HostError::Resident(error) => match error {
            RegistryError::InvalidLimits
            | RegistryError::CurrentUnit
            | RegistryError::InstructionOverlap { .. } => 7,
            RegistryError::Allocation | RegistryError::IdentityExhausted => 9,
            RegistryError::WrongAddressSpace
            | RegistryError::InvalidUnit
            | RegistryError::NotFound { .. } => 3,
            RegistryError::UnitCapacity | RegistryError::ByteCapacity => 18,
            RegistryError::Compile(_) => 10,
            RegistryError::CodeInvalidated => 4,
        },
        HostError::GenerationExhausted | HostError::Infrastructure => 9,
        HostError::Call(error) => match error {
            CallError::Busy => 12,
            CallError::InvalidStop => 13,
            CallError::InvalidToken => 14,
            CallError::StateChanged => 15,
            CallError::Cancelled => 16,
            CallError::InvalidRequest => 7,
            CallError::TokenExhausted => 9,
            CallError::Memory(_) => 8,
        },
        HostError::Media(error) => media::status(error),
        HostError::User32(error) => {
            use crate::windows::user32::User32Error32 as U;
            match error {
                U::Busy => 12,
                U::Capacity => 18,
                U::Allocation => 9,
                U::InvalidToken => 14,
                U::Memory(_) => 8,
                U::Window(
                    crate::windows::window::WindowError32::QueueFull
                    | crate::windows::window::WindowError32::Capacity,
                ) => 18,
                U::Window(
                    crate::windows::window::WindowError32::Exhausted
                    | crate::windows::window::WindowError32::Allocation,
                ) => 9,
                _ => 7,
            }
        }
        HostError::D3D8(error) => {
            use crate::{
                graphics::d3d8::{
                    commands::CommandError as C, device::DeviceError as D,
                    resources::ResourceError as R, shader::registry::RegistryError as S,
                },
                process::D3D8Error32 as E,
            };
            match error {
                E::NotConfigured => 38,
                E::AlreadyConfigured => 39,
                E::InvalidConfiguration => 7,
                E::Allocation => 9,
                E::InvalidDevice | E::StaleReply => 40,
                E::Busy => 12,
                E::Device(error) => match error {
                    D::Lost => 44,
                    D::NotReset => 45,
                    D::Removed => 46,
                    D::NotAvailable | D::Resource(R::NotAvailable) => 20,
                    D::PendingReadback
                    | D::Resource(R::ReadbackRequired)
                    | D::Command(C::Backpressure) => 12,
                    D::StaleReadback
                    | D::Resource(R::StaleResource)
                    | D::Command(C::StaleBatch | C::StalePreparation) => 40,
                    D::Allocation
                    | D::IdentityExhausted
                    | D::Resource(R::Allocation | R::IdentityExhausted)
                    | D::Command(C::Allocation | C::IdentityExhausted)
                    | D::Shader(S::Allocation | S::IdentityExhausted) => 9,
                    D::Capacity
                    | D::Resource(R::ByteCapacity | R::ResourceCapacity)
                    | D::Command(C::Capacity | C::OutputTooSmall)
                    | D::Shader(S::ObjectCapacity | S::ByteCapacity) => 18,
                    _ => 7,
                },
            }
        }
    }
}

pub(super) fn cpu_store(
    operation: impl FnOnce(&mut EngineInstance) -> Result<StoreCompletion, HostError>,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match operation(instance) {
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        }
    })
}

mod modules;
pub(crate) use modules::{
    abort_module_input, abort_module_operation, append_module_input, begin_module_input,
    commit_module_input, configure_module_limits, detach_modules_for_exit,
    finish_module_initializer, free_library_module, load_library_module, module_gates,
    module_handle, module_initializer, module_teb, register_kernel32_module, resolve_module_export,
    start_module_process,
};

mod loading;
pub(crate) use loading::{
    abort_image_input, append_image_input, begin_image_input, load_pe32, load_pe32_at,
    load_pe32_input_at, load_pe32_linked_at, load_pe32_linked_v2_at, load_pe32_linked_v2_input_at,
    load_pe32_linked_v3_input_at, load_pe32_linked_v4_input_at, load_pe32_linked_v5_input_at,
    load_pe32_linked_v6_input_at, load_pe32_linked_v7_input_at, load_pe32_linked_v8_input_at,
    start_loaded_image,
};

mod execution;
pub(crate) use execution::{
    acknowledge_resident_callback_installation, acknowledge_resident_installation, compile,
    compile_entries, compile_resident, compile_resident_callback_unit, compile_resident_entries,
    compile_resident_with_gates, compile_stopped_entry, compile_stopped_resident_entry,
    compile_with_gates, discard_installed_resident, discard_unacknowledged_resident,
    dispatcher_module, find_installed_resident, find_resident, generation, guard,
    guard_dispatch_entry, guard_resident, module_len, module_ptr, resident_module,
    retire_stale_resident,
};

mod calls;
pub(crate) use calls::{
    abandon_call, abort_callback, authorize_resident_callback, begin_callback,
    begin_resident_callback, capture_active_resident_callback_call, capture_call,
    capture_resident_call, capture_resident_callback_call, complete_active_resident_callback_call,
    complete_active_resident_callback_windows_call, complete_call, complete_resident_call,
    complete_resident_callback_call, complete_resident_windows_call, complete_windows_call,
    finish_callback, finish_resident_callback, resume_callback_code, resume_callback_entries,
    select_resident_callback_unit,
};

mod memory;
pub(crate) use memory::{
    cpu_atomic_and_resident_u32, cpu_atomic_and_u32, cpu_atomic_or_resident_u32, cpu_atomic_or_u32,
    cpu_atomic_xor_resident_u32, cpu_atomic_xor_u32, cpu_bit_test_reset_resident32,
    cpu_bit_test_reset32, cpu_bit_test_set_resident32, cpu_bit_test_set32,
    cpu_compare_exchange_resident8, cpu_compare_exchange_resident16,
    cpu_compare_exchange_resident32, cpu_compare_exchange8, cpu_compare_exchange16,
    cpu_compare_exchange32, cpu_exchange_add_resident8, cpu_exchange_add_resident16,
    cpu_exchange_add_resident32, cpu_exchange_add8, cpu_exchange_add16, cpu_exchange_add32,
    cpu_exchange_resident8, cpu_exchange_resident16, cpu_exchange_resident32, cpu_exchange8,
    cpu_exchange16, cpu_exchange32, cpu_store_resident8, cpu_store_resident16,
    cpu_store_resident32, cpu_store8, cpu_store16, cpu_store32,
};

pub(crate) use memory::{
    atomic_and_resident_u32, atomic_and_u32, atomic_or_resident_u32, atomic_or_u32,
    atomic_xor_resident_u32, atomic_xor_u32, bit_test_reset_resident32, bit_test_reset32,
    bit_test_set_resident32, bit_test_set32, compare_exchange_resident8,
    compare_exchange_resident16, compare_exchange_resident32, compare_exchange8,
    compare_exchange16, compare_exchange32, exchange_add_resident8, exchange_add_resident16,
    exchange_add_resident32, exchange_add8, exchange_add16, exchange_add32, exchange_resident8,
    exchange_resident16, exchange_resident32, exchange8, exchange16, exchange32, map, protect,
    read8, read16, read32, store_resident8, store_resident16, store_resident32, store8, store16,
    store32, unmap, upload, write_words32, write8, write32,
};

mod xstate;
pub(crate) use xstate::{
    cpu_xrstor, cpu_xrstor_avx, cpu_xrstor_avx_resident, cpu_xrstor_resident, cpu_xsave,
    cpu_xsave_avx, cpu_xsave_avx_resident, cpu_xsave_resident,
};

mod cpu;
pub(crate) use cpu::{
    configure_segment_descriptor, configure_segment_descriptor_raw, configure_system_register_bank,
    cpu_fxsave, cpu_fxsave_resident, cpu_integer, cpu_integer_resident, cpu_mask_write,
    cpu_mask_write_resident, cpu_read, cpu_read8, cpu_read16, cpu_read32, cpu_write,
    cpu_write_resident, far_call, far_call_resident, far_jump, far_jump_resident, far_return,
    far_return_resident, segment_load, segment_load_far, segment_load_far_resident,
    segment_load_resident, set_segment_base, simd_execute, simd_execute_resident, stack_execute,
    stack_execute_resident, x87_op, x87_resident_op,
};

mod paging;
pub(crate) use paging::{
    cpu_fetch_fault, cpu_fetch_fault_resident, cpu_read_modify, cpu_read_modify8,
    cpu_read_modify16, cpu_read_modify32, read_cr2,
};

mod detection;
pub(crate) use detection::{
    finish_exception_detection, guard_detect, guard_detect_resident, take_cpu_event,
};

mod avx;
pub(crate) use avx::{avx_execute, avx_execute_resident};
