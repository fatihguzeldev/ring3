#![forbid(unsafe_code)]

use std::cell::RefCell;

use super::{CallError, EngineInstance, HostError, StoreCompletion};
use crate::cpu::dbt::RegistryError;

#[cfg(feature = "external-cpu")]
mod component_memory;
#[cfg(feature = "external-cpu")]
pub(crate) use component_memory::component_memory_lease;
#[cfg(feature = "external-cpu")]
mod component_cpu;
#[cfg(feature = "external-cpu")]
pub(crate) use component_cpu::{
    component_cpu_ack_mappings, component_cpu_attach, component_cpu_collect_dirty,
    component_cpu_dirty, component_cpu_gate, component_cpu_publish, component_cpu_release,
    component_cpu_view, component_cpu_write_fault,
};

mod clock;
mod core;
pub(crate) use clock::{advance_cpu_clock, configure_cpu_clock, observe_cpu_clock};
#[cfg(feature = "observability")]
pub(crate) use core::core_cold_status_ptr;
pub(crate) use core::{
    core_begin, core_begin_windows_callback, core_cache_format, core_cache_status_ptr,
    core_cache_victim, core_capture_windows_call, core_chain_cold, core_chain_next,
    core_chain_probe, core_chain_resolve_executable_owned, core_chain_resolve_owned,
    core_chain_select, core_code_len, core_code_ptr, core_cold_run, core_complete_windows_call,
    core_context_ptr, core_continuation_format, core_continuation_ptr, core_dispatch_code_len,
    core_dispatch_code_ptr, core_dispatch_exception, core_end, core_end_accounted_owned,
    core_end_owned, core_export, core_finish_exception, core_finish_windows_callback, core_import,
    core_link, core_link_owners_ptr, core_lookup, core_lookup_executable, core_numeric,
    core_numeric_owned, core_precise, core_precise_owned, core_prepare, core_prepare_executable,
    core_read, core_recovery_scratch_ptr, core_trace_finish, core_trace_pointer, core_trace_ready,
    core_trace_root, core_trace_start, core_trace_status_ptr, core_unlink, core_write,
};

#[cfg(feature = "observability")]
mod observability;
#[cfg(feature = "observability")]
pub(crate) use observability::{
    configure_observability, observability_drain, observability_finish, observability_status,
};

mod media;
pub(crate) use media::*;
mod windows;
pub(crate) use windows::*;

struct Registry {
    opened: bool,
    instance: Option<EngineInstance>,
    core: Option<core::CoreSession32>,
}

impl Registry {
    fn core_active(&self) -> bool {
        self.core
            .as_ref()
            .is_some_and(core::CoreSession32::is_active)
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = const {
        RefCell::new(Registry { opened: false, instance: None, core: None })
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
        if registry
            .core
            .as_ref()
            .is_some_and(core::CoreSession32::is_active)
        {
            return 12;
        }
        #[cfg(feature = "external-cpu")]
        if registry
            .instance
            .as_ref()
            .is_some_and(|instance| instance.external_cpu_memory_pinned)
        {
            return 12;
        }
        registry.core = None;
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
    mutate_inner(operation, false)
}

// only the native terminal method can upgrade a private light seal. it validates
// key/token before materializing, revokes on valid-token attempts, and keeps a
// failed frame pinned for the existing full-seal retry protocol.
pub(super) fn mutate_execution_terminal(
    operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>,
) -> u32 {
    mutate_inner(operation, true)
}

fn mutate_inner(
    operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>,
    terminal: bool,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry
            .core
            .as_ref()
            .is_some_and(core::CoreSession32::is_active)
        {
            return 12;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        if !terminal {
            if let Err(error) = instance.guard_compiled_public_transition() {
                return status(error);
            }
            instance.invalidate_owned_context();
        } else if instance.compiled_execution.is_none() {
            // preserve checked/c1 adapter behavior. compiled key/token errors
            // must not change even a seeded full continuation's private state.
            instance.invalidate_owned_context();
        }
        operation(instance).map_or_else(status, |()| 0)
    })
}

pub(super) fn status(error: HostError) -> u32 {
    match error {
        HostError::Comdlg(error) => match error {
            crate::windows::commdlg::ComdlgError32::Allocation => 9,
            crate::windows::commdlg::ComdlgError32::Capacity => 18,
            _ => 7,
        },
        HostError::Gdi(error) => match error {
            crate::windows::gdi::GdiError32::Allocation => 9,
            crate::windows::gdi::GdiError32::Capacity => 18,
            _ => 7,
        },
        HostError::Version(_)
        | HostError::Shlwapi(_)
        | HostError::Shell(_)
        | HostError::Automation(_) => 7,
        HostError::Activation(error) => match error {
            crate::windows::activation::ActivationError32::NotEnoughMemory => 9,
            crate::windows::activation::ActivationError32::Busy => 12,
            _ => 7,
        },
        HostError::Resource(error) => match error {
            crate::process::ResourceError32::Allocation => 9,
            _ => 7,
        },
        HostError::Exception(error) => match error {
            crate::windows::exceptions::ExceptionError32::Allocation
            | crate::windows::exceptions::ExceptionError32::Exhausted => 9,
            crate::windows::exceptions::ExceptionError32::Busy => 12,
            crate::windows::exceptions::ExceptionError32::Stale => 40,
            _ => 7,
        },
        HostError::Nls(error) => match error {
            crate::windows::nls::NlsError32::Allocation => 9,
            crate::windows::nls::NlsError32::Capacity => 18,
            _ => 7,
        },
        HostError::Registry(error) => match error {
            crate::windows::registry::RegistryError32::NotEnoughMemory => 9,
            crate::windows::registry::RegistryError32::Busy => 12,
            crate::windows::registry::RegistryError32::StaleReceipt => 40,
            _ => 7,
        },
        HostError::Thread(error) => match error {
            crate::windows::thread::ThreadError32::Capacity => 18,
            crate::windows::thread::ThreadError32::Allocation
            | crate::windows::thread::ThreadError32::Exhausted => 9,
            crate::windows::thread::ThreadError32::Busy => 12,
            crate::windows::thread::ThreadError32::Unsupported => 20,
            _ => 7,
        },
        HostError::WindowsMemory(error) => match error {
            crate::windows::memory::MemoryError32::Capacity => 18,
            crate::windows::memory::MemoryError32::Allocation
            | crate::windows::memory::MemoryError32::Exhausted => 9,
            crate::windows::memory::MemoryError32::Busy => 12,
            crate::windows::memory::MemoryError32::Unsupported => 20,
            _ => 7,
        },
        HostError::WindowsProcess(error) => match error {
            crate::windows::process::ProcessError32::Capacity => 18,
            crate::windows::process::ProcessError32::Allocation => 9,
            crate::windows::process::ProcessError32::NoAccess => 8,
            _ => 7,
        },
        HostError::Ole32(error) => match error {
            crate::process::Ole32Error32::Allocation => 9,
            crate::process::Ole32Error32::Busy => 12,
            _ => 7,
        },
        HostError::File(error) => match error {
            crate::files::FileError32::Stale => 40,
            crate::files::FileError32::Capacity => 18,
            crate::files::FileError32::Allocation | crate::files::FileError32::Exhausted => 9,
            crate::files::FileError32::Busy => 12,
            crate::files::FileError32::Unsupported => 20,
            _ => 7,
        },
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
        HostError::DSound(error) => {
            use crate::process::DSoundError32 as D;
            match error {
                D::NotConfigured => 38,
                D::AlreadyConfigured => 39,
                D::Allocation => 9,
                D::InvalidConfiguration | D::InvalidRequest => 7,
            }
        }
        HostError::DirectInput(error) => {
            use crate::process::DirectInputError32 as D;
            match error {
                D::NotConfigured => 38,
                D::AlreadyConfigured => 39,
                D::Allocation => 9,
                D::Busy => 12,
                D::InvalidToken => 14,
                D::Memory(_) => 8,
                D::Input(error) => media::status(crate::process::MediaError::Input(error)),
                D::Synchronization(error) => status(HostError::Synchronization(error)),
                D::InvalidConfiguration | D::InvalidRequest => 7,
            }
        }
        HostError::Synchronization(error) => {
            use crate::windows::synchronization::SynchronizationError32 as S;
            match error {
                S::Stale => 40,
                S::Capacity => 18,
                S::Allocation | S::Exhausted => 9,
                S::Busy => 12,
                S::Unsupported => 20,
                _ => 7,
            }
        }
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
        if registry
            .core
            .as_ref()
            .is_some_and(core::CoreSession32::is_active)
        {
            return 12;
        }
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
    module_gates_page, module_handle, module_initializer, module_teb, pending_module_source,
    register_kernel32_module, resolve_module_export, resume_module_source, start_module_process,
};

mod loading;
pub(crate) use loading::{
    abort_image_input, append_image_input, begin_image_input, load_pe32, load_pe32_at,
    load_pe32_input_at, load_pe32_linked_at, load_pe32_linked_v2_at, load_pe32_linked_v2_input_at,
    load_pe32_linked_v3_input_at, load_pe32_linked_v4_input_at, load_pe32_linked_v5_input_at,
    load_pe32_linked_v6_input_at, load_pe32_linked_v7_input_at, load_pe32_linked_v8_input_at,
    start_loaded_image,
};

#[cfg(feature = "observability")]
mod census;
mod execution;
#[cfg(feature = "observability")]
pub(crate) use census::{begin_execution_census, execution_census_page, stop_execution_census};
pub(crate) use execution::{
    acknowledge_cache_eviction, acknowledge_resident_callback_installation,
    acknowledge_resident_installation, begin_compiled_execution_receipt, begin_execution_receipt,
    begin_owned_execution_receipt, cache_usage, cancel_cache_request, compile, compile_entries,
    compile_resident, compile_resident_callback_unit, compile_resident_entries,
    compile_resident_with_gates, compile_stopped_entry, compile_stopped_resident_entry,
    compile_with_gates, compiled_execution_usage, configure_execution,
    configure_execution_with_limits, discard_installed_resident, discard_unacknowledged_resident,
    dispatcher_module, find_installed_resident, find_resident, finish_execution_receipt,
    generation, guard, guard_dispatch_entry, guard_resident, install_cached_entry, module_len,
    module_ptr, pending_cache_request, prepare_cache_entry, prepare_cache_promotion,
    resident_module, retire_stale_resident,
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
    cpu_mask_write_resident, cpu_read, cpu_read8, cpu_read16, cpu_read32, cpu_read32_view,
    cpu_write, cpu_write_resident, far_call, far_call_resident, far_jump, far_jump_resident,
    far_return, far_return_resident, segment_load, segment_load_far, segment_load_far_resident,
    segment_load_resident, set_segment_base, simd_execute, simd_execute_resident, stack_execute,
    stack_execute_resident, x87_op, x87_resident_group, x87_resident_group32, x87_resident_op,
    x87_resident_pair,
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
