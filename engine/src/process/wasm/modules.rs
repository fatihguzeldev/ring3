use super::mutate;
use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        modules::{self, ModuleRecord32},
    },
    loader::{ImportSymbol32, ModuleError32, ModuleLimits32, ParserLimits32},
    process::{
        EngineInstance, HostError, ModuleInitializerKind32, ModuleOperation32,
        ModuleRuntimeConfig32,
    },
};

pub(crate) fn configure_module_limits(
    modules: u32,
    source_bytes: u32,
    forwarder_depth: u32,
    metadata_entries: u32,
    string_bytes: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_module_limits(ModuleLimits32 {
            modules: modules as usize,
            source_bytes: source_bytes as usize,
            forwarder_depth: forwarder_depth as usize,
            parser: ParserLimits32 {
                metadata_entries: metadata_entries as usize,
                string_bytes: string_bytes as usize,
            },
        })
    })
}

pub(crate) fn register_kernel32_module(base: u32) -> u32 {
    mutate(|instance| instance.register_kernel32_module(base))
}

pub(crate) fn begin_module_input(total: u32, name_length: u32) -> u32 {
    mutate(|instance| {
        let name = input_string(instance, name_length)?;
        instance.begin_module_input(total, &name)
    })
}

pub(crate) fn append_module_input(offset: u32, length: u32) -> u32 {
    mutate(|instance| {
        instance.memory()?;
        if length == 0 || length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = [0; TRANSFER_SIZE];
        bytes[..length as usize]
            .copy_from_slice(&instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize]);
        instance.append_module_input(offset, &bytes[..length as usize])
    })
}

pub(crate) fn commit_module_input() -> u32 {
    mutate(EngineInstance::commit_module_input)
}
pub(crate) fn abort_module_input() -> u32 {
    mutate(EngineInstance::abort_module_input)
}

pub(crate) fn start_module_process(
    name_length: u32,
    actual_base: u32,
    stack_base: u32,
    stack_pages: u32,
    runtime_base: u32,
) -> u32 {
    mutate(|instance| {
        let name = input_string(instance, name_length)?;
        let result = instance.start_module_process(
            &name,
            optional_base(actual_base),
            ModuleRuntimeConfig32 {
                stack_base,
                stack_pages,
                runtime_base,
            },
        )?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn load_library_module(name_length: u32, actual_base: u32) -> u32 {
    mutate(|instance| {
        let name = input_string(instance, name_length)?;
        let result = instance.load_library_module(&name, optional_base(actual_base))?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn resolve_module_export(handle: u32, symbol_kind: u32, value: u32) -> u32 {
    mutate(|instance| {
        let symbol = match symbol_kind {
            0 => ImportSymbol32::Name(input_string(instance, value)?),
            1 if value <= u16::MAX as u32 => ImportSymbol32::Ordinal(value as u16),
            _ => return Err(HostError::InvalidRequest),
        };
        let result = instance.resolve_module_export(handle, &symbol)?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn free_library_module(handle: u32) -> u32 {
    mutate(|instance| {
        let result = instance.free_library_module(handle)?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn detach_modules_for_exit() -> u32 {
    mutate(|instance| {
        let result = instance.detach_modules_for_exit()?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn module_initializer() -> u32 {
    mutate(|instance| {
        let result = instance
            .module_initializer()?
            .map(ModuleOperation32::Pending);
        operation_record(instance, result)
    })
}

pub(crate) fn finish_module_initializer(token_low: u32, token_high: u32) -> u32 {
    mutate(|instance| {
        let token = u64::from(token_low) | (u64::from(token_high) << 32);
        let result = instance.finish_module_initializer(token)?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn abort_module_operation() -> u32 {
    mutate(|instance| {
        let result = instance.abort_module_operation()?;
        operation_record(instance, Some(result))
    })
}

pub(crate) fn module_handle(name_length: u32) -> u32 {
    mutate(|instance| {
        let name = if name_length == 0 {
            None
        } else {
            Some(input_string(instance, name_length)?)
        };
        let handle = instance.module_handle(name.as_deref())?.unwrap_or(0);
        operation_record(instance, Some(ModuleOperation32::Loaded { handle }))
    })
}

pub(crate) fn module_teb() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let handle = instance.module_teb_address().unwrap_or(0);
        operation_record(instance, Some(ModuleOperation32::Loaded { handle }))
    })
}

pub(crate) fn module_gates() -> u32 {
    mutate(|instance| {
        let mut gates = instance.module_gates()?;
        gates.sort_unstable_by_key(|gate| gate.entry.0);
        let size =
            modules::module_gates_size(gates.len()).map_err(|_| HostError::Infrastructure)?;
        if size > TRANSFER_SIZE {
            return Err(HostError::Module(ModuleError32::Capacity));
        }
        modules::encode_module_gates(
            &gates,
            &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + size],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn module_gates_page(start: u32, maximum: u32) -> u32 {
    mutate(|instance| {
        let capacity = (TRANSFER_SIZE - modules::MODULE_GATE_PAGE_HEADER_SIZE) / 8;
        if maximum == 0 || maximum as usize > capacity {
            return Err(HostError::InvalidRequest);
        }
        let mut gates = instance.module_gates()?;
        gates.sort_unstable_by_key(|gate| gate.entry.0);
        let total = u32::try_from(gates.len()).map_err(|_| HostError::Infrastructure)?;
        if start > total {
            return Err(HostError::InvalidRequest);
        }
        let count = maximum.min(total - start) as usize;
        let size = modules::MODULE_GATE_PAGE_HEADER_SIZE + count * 8;
        modules::encode_module_gate_page(
            &gates[start as usize..start as usize + count],
            total,
            start,
            &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + size],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

fn optional_base(base: u32) -> Option<u32> {
    (base != 0).then_some(base)
}

fn input_string(instance: &EngineInstance, length: u32) -> Result<String, HostError> {
    instance.memory()?;
    if length == 0 || length as usize > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    let bytes = &instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::InvalidRequest)?;
    if text.bytes().any(|byte| byte == 0) {
        return Err(HostError::InvalidRequest);
    }
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
    result.push_str(text);
    Ok(result)
}

fn operation_record(
    instance: &mut EngineInstance,
    result: Option<ModuleOperation32>,
) -> Result<(), HostError> {
    let record = match result {
        None => ModuleRecord32::default(),
        Some(ModuleOperation32::Pending(init)) => ModuleRecord32 {
            outcome: 1,
            token: init.token,
            module: init.module.get(),
            entry: init.entry,
            return_pc: init.return_pc,
            return_id: init.return_id,
            return_esp: init.return_esp,
            reason: init.reason,
            kind: match init.kind {
                ModuleInitializerKind32::TlsCallback => 1,
                ModuleInitializerKind32::DllMain => 2,
            },
            ..Default::default()
        },
        Some(ModuleOperation32::NeedBytes { request }) => ModuleRecord32 {
            outcome: 6,
            value: request.length,
            token: request.request,
            module: request.file.0,
            ..Default::default()
        },
        Some(ModuleOperation32::Loaded { handle }) => ModuleRecord32 {
            outcome: 2,
            value: handle,
            ..Default::default()
        },
        Some(ModuleOperation32::Released) => ModuleRecord32 {
            outcome: 3,
            ..Default::default()
        },
        Some(ModuleOperation32::InitializationFailed) => ModuleRecord32 {
            outcome: 4,
            ..Default::default()
        },
        Some(ModuleOperation32::ExportResolved { address }) => ModuleRecord32 {
            outcome: 5,
            value: address,
            ..Default::default()
        },
    };
    modules::encode_module(
        &record,
        &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + modules::MODULE_RECORD_SIZE],
    )
    .map_err(|_| HostError::Infrastructure)
}

pub(crate) fn pending_module_source() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let record = instance
            .pending_module_source()
            .map(|wait| modules::ModuleSourceRecord32 {
                epoch: wait.epoch,
                token: wait.token,
                thread: wait.thread,
                ready: wait.ready,
                request: wait.request,
            });
        modules::encode_module_source(
            record,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + modules::MODULE_SOURCE_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
pub(crate) fn resume_module_source(
    epoch_low: u32,
    epoch_high: u32,
    token_low: u32,
    token_high: u32,
) -> u32 {
    mutate(|instance| {
        let operation = instance.resume_module_source(
            u64::from(epoch_low) | u64::from(epoch_high) << 32,
            u64::from(token_low) | u64::from(token_high) << 32,
        )?;
        operation_record(instance, Some(operation))
    })
}
