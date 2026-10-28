use super::{REGISTRY, status};
use crate::{
    abi::{arena::*, external_memory::ExternalMemory, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

fn access(
    low: u32,
    high: u32,
    attached: bool,
    operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>,
) -> u32 {
    let key = u64::from(low) | u64::from(high) << 32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.core_active() {
            return 12;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        if !instance.is_open() {
            return 5;
        }
        if instance.key() != key {
            return 3;
        }
        if attached && !instance.external_cpu_memory_pinned {
            return 12;
        }
        operation(instance).map_or_else(status, |()| 0)
    })
}

pub(crate) fn component_cpu_attach(low: u32, high: u32, backing: ExternalMemory) -> u32 {
    access(low, high, false, |instance| {
        if instance.external_cpu_memory_pinned
            || instance.image_input.is_some()
            || instance.pending_call.is_some()
            || instance.callback.is_some()
            || instance.cpu_event.is_some()
            || instance.execution_frame.is_some()
            || !instance.external_cpu_module_ready()
            || instance.core_owner.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        instance.enable_core_execution()?;
        if let Err(error) = instance
            .memory
            .as_mut()
            .unwrap()
            .attach_external_backing(backing)
        {
            instance.core_owner = None;
            return Err(HostError::Memory(error));
        }
        instance.external_cpu_memory_pinned = true;
        Ok(())
    })
}

pub(crate) fn component_cpu_view(low: u32, high: u32) -> u32 {
    access(low, high, false, |instance| {
        let bytes = instance
            .memory
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .external_descriptor();
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(&bytes);
        Ok(())
    })
}

pub(crate) fn component_cpu_dirty(low: u32, high: u32, count: u32) -> u32 {
    access(low, high, true, |instance| {
        if count > (TRANSFER_SIZE / 4) as u32 {
            return Err(HostError::InvalidRequest);
        }
        let mut slots = [0; TRANSFER_SIZE / 4];
        for (index, slot) in slots[..count as usize].iter_mut().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            *slot = u32::from_le_bytes(instance.arena()[at..at + 4].try_into().unwrap());
        }
        instance
            .memory
            .as_mut()
            .unwrap()
            .publish_external_dirty(&slots[..count as usize])
            .map_err(HostError::Memory)
    })
}

fn gate(instance: &EngineInstance, pc: u32) -> Option<u32> {
    let gate = crate::process::execution::cache::cache_gate_at(
        &instance.modules,
        instance.callback.as_ref(),
        pc,
    )?;
    let mut bytes = [0; 2];
    instance
        .memory
        .as_ref()?
        .fetch(GuestAddress(pc), &mut bytes)
        .ok()?;
    (bytes == [0x0f, 0x0b]).then_some(gate.id)
}

pub(crate) fn component_cpu_gate(low: u32, high: u32, pc: u32) -> u32 {
    access(low, high, true, |instance| {
        let id = gate(instance, pc).unwrap_or(0);
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&id.to_le_bytes());
        Ok(())
    })
}

pub(crate) fn component_cpu_publish(
    low: u32,
    high: u32,
    stop: u32,
    detail: u32,
    auxiliary: u32,
    vector: u32,
    retired: u32,
    work: u32,
) -> u32 {
    access(low, high, true, |instance| {
        instance.guard_core_entry()?;
        if retired > work {
            return Err(HostError::InvalidRequest);
        }
        let arena = instance.arena();
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let (mut reason, origin) = super::core::exit_from_words(stop, detail, auxiliary, vector)?;
        if stop == 7 {
            let id = gate(instance, state.eip).ok_or(HostError::InvalidRequest)?;
            if vector != 0 && vector != id {
                return Err(HostError::InvalidRequest);
            }
            reason = ExitReason::Gate { id };
        }
        let x87 = decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let simd = decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let ymm = decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let segments = instance.segment_context()?;
        let instruction = (stop == 10).then_some(crate::process::CpuInstructionSite32 {
            eip: detail,
            retired: retired.saturating_sub(1),
            flags: state.eflags,
            ss_debug_shadow: false,
            work_consumed: Some(work.saturating_sub(1)),
        });
        let prepared = instance.prepare_core_exit(
            state,
            ExecutionExit { retired, reason },
            work,
            origin,
            instruction,
            (x87, simd, ymm, segments),
        )?;
        instance.commit_core_exit(prepared);
        Ok(())
    })
}

pub(crate) fn component_cpu_write_fault(low: u32, high: u32, address: u32) -> u32 {
    access(low, high, true, |instance| {
        instance
            .memory
            .as_mut()
            .unwrap()
            .external_write_fault(address)
            .map_err(HostError::Memory)
    })
}

pub(crate) fn component_cpu_release(low: u32, high: u32) -> u32 {
    access(low, high, true, |instance| {
        instance
            .memory
            .as_mut()
            .unwrap()
            .release_external_backing()
            .map_err(HostError::Memory)?;
        instance.external_cpu_memory_pinned = false;
        Ok(())
    })
}
