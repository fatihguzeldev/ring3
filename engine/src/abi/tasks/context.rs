use crate::{
    abi::{
        AbiError, header,
        x86::{
            STATE_SIZE, StateMetadata, decode_state, decode_state_metadata, encode_state_current,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext, SegmentDescriptorRecord, SegmentState,
        tasks::{TaskContextImage32, TaskIncomingContext32, TaskTransitionFault32},
    },
};

pub const TASK_CONTEXT_SIZE: usize = 184;

/// loading has no known incoming State32, selectors or CPL; zero bytes never mean old-task state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskContextRecord32 {
    Absent,
    Loading {
        revision: u64,
    },
    Loaded {
        revision: u64,
        context: TaskIncomingContext32,
        ss_debug_shadow: bool,
    },
}

pub fn encode_task_context(record: TaskContextRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != TASK_CONTEXT_SIZE {
        return Err(AbiError::Length);
    }
    let mut bytes = [0; TASK_CONTEXT_SIZE];
    header::write(&mut bytes, *b"R3TC");
    let revision = match record {
        TaskContextRecord32::Absent => 0,
        TaskContextRecord32::Loading { revision } => {
            if revision == 0 {
                return Err(AbiError::TaskContext);
            }
            revision
        }
        TaskContextRecord32::Loaded {
            revision,
            context,
            ss_debug_shadow,
        } => {
            if revision == 0 || TaskIncomingContext32::from_image(context.image()).is_none() {
                return Err(AbiError::TaskContext);
            }
            let mut state = [0; STATE_SIZE];
            encode_state_current(&context.state(), &mut state)?;
            StateMetadata { ss_debug_shadow }.write(&mut state);
            bytes[16..16 + STATE_SIZE].copy_from_slice(&state);
            for (offset, value) in [
                (72, u32::from(context.loaded_mask())),
                (
                    76,
                    0x1f | if context.ldt_cache().is_some() {
                        0x20
                    } else {
                        0
                    } | if context.task_trap() { 0x40 } else { 0 },
                ),
                (88, u32::from(context.current_privilege()) | 0x3f00),
                (92, u32::from(context.ldtr())),
            ] {
                header::write_u32(&mut bytes, offset, value);
            }
            if let Some(fault) = context.fault() {
                let (kind, code) = match fault {
                    TaskTransitionFault32::InvalidTss(code) => (1, code),
                    TaskTransitionFault32::NotPresent(code) => (2, code),
                    TaskTransitionFault32::StackSegment(code) => (3, code),
                    TaskTransitionFault32::GeneralProtection(code) => (4, code),
                };
                header::write_u32(&mut bytes, 80, kind);
                header::write_u32(&mut bytes, 84, code);
            }
            let segments = context.segment_image();
            for index in 0..6 {
                header::write_u32(
                    &mut bytes,
                    96 + index * 4,
                    u32::from(segments.state.selectors[index]),
                );
                header::write_u32(&mut bytes, 120 + index * 4, segments.state.bases[index]);
                header::write_u32(&mut bytes, 144 + index * 4, segments.caches[index].bits());
            }
            if let Some(record) = context.ldt_cache() {
                let (low, high) = record.to_words();
                header::write_u32(&mut bytes, 168, low);
                header::write_u32(&mut bytes, 172, high);
            }
            revision
        }
    };
    bytes[176..184].copy_from_slice(&revision.to_le_bytes());
    output.copy_from_slice(&bytes);
    Ok(())
}

pub fn decode_task_context(bytes: &[u8]) -> Result<TaskContextRecord32, AbiError> {
    header::validate(bytes, *b"R3TC", TASK_CONTEXT_SIZE)?;
    let invalid = AbiError::TaskContext;
    let revision = u64::from_le_bytes(bytes[176..184].try_into().unwrap());
    if revision == 0 || header::read_u32(bytes, 76) == 0 {
        if bytes[16..176].iter().any(|&byte| byte != 0) {
            return Err(invalid);
        }
        return Ok(if revision == 0 {
            TaskContextRecord32::Absent
        } else {
            TaskContextRecord32::Loading { revision }
        });
    }
    let flags = header::read_u32(bytes, 76);
    if flags & !0x7f != 0 || flags & 0x1f != 0x1f {
        return Err(invalid);
    }
    let loaded = u8::try_from(header::read_u32(bytes, 72)).map_err(|_| invalid)?;
    let ldtr = u16::try_from(header::read_u32(bytes, 92)).map_err(|_| invalid)?;
    let mut segments = SegmentContext {
        state: SegmentState {
            selectors: [0; 6],
            bases: [0; 6],
        },
        caches: [SegmentCache::UNUSABLE; 6],
    };
    for index in 0..6 {
        segments.state.selectors[index] =
            u16::try_from(header::read_u32(bytes, 96 + index * 4)).map_err(|_| invalid)?;
        segments.state.bases[index] = header::read_u32(bytes, 120 + index * 4);
        segments.caches[index] =
            SegmentCache::from_bits(header::read_u32(bytes, 144 + index * 4)).ok_or(invalid)?;
    }
    if header::read_u32(bytes, 88) != (0x3f00 | u32::from(segments.state.selectors[1] & 3)) {
        return Err(invalid);
    }
    let fault = match (header::read_u32(bytes, 80), header::read_u32(bytes, 84)) {
        (0, 0) => None,
        (1, code) => Some(TaskTransitionFault32::InvalidTss(code)),
        (2, code) => Some(TaskTransitionFault32::NotPresent(code)),
        (3, code) => Some(TaskTransitionFault32::StackSegment(code)),
        (4, code) => Some(TaskTransitionFault32::GeneralProtection(code)),
        _ => return Err(invalid),
    };
    let ldt = SegmentDescriptorRecord::from_words(
        header::read_u32(bytes, 168),
        header::read_u32(bytes, 172),
    );
    let ldt_cache = if flags & 0x20 != 0 {
        Some(ldt)
    } else {
        if ldt.to_words() != (0, 0) {
            return Err(invalid);
        }
        None
    };
    let state = decode_state(&bytes[16..16 + STATE_SIZE])?;
    let context = TaskIncomingContext32::from_image(TaskContextImage32 {
        state,
        segments,
        loaded,
        ldtr,
        ldt_cache,
        task_trap: flags & 0x40 != 0,
        fault,
    })
    .ok_or(invalid)?;
    let ss_debug_shadow = decode_state_metadata(&bytes[16..16 + STATE_SIZE])?.ss_debug_shadow;
    Ok(TaskContextRecord32::Loaded {
        revision,
        context,
        ss_debug_shadow,
    })
}

#[cfg(test)]
mod tests;
