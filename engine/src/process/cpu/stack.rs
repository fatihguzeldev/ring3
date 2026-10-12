use crate::process::{EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::{SegmentAccess, SegmentContext, SegmentRegister, StackFrameKind},
    memory::{Access, AddressSpace, GuestAddress, MemoryError},
};

struct Prepared {
    registers: [u32; 8],
    addresses: [u32; 32],
    values: [u32; 32],
    count: usize,
}

impl Prepared {
    fn push(&mut self, memory: &AddressSpace, width: u32, value: u32) -> Result<(), MemoryError> {
        let address = self.registers[4].wrapping_sub(width);
        memory.check_span(GuestAddress(address), width as usize, Access::Write)?;
        self.addresses[self.count] = address;
        self.values[self.count] = value;
        self.count += 1;
        self.registers[4] = address;
        Ok(())
    }

    fn read(&self, memory: &AddressSpace, address: u32, width: u32) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        memory.read(GuestAddress(address), &mut bytes[..width as usize])?;
        // ENTER's ordered reads must observe a preceding push when frames overlap.
        for index in 0..self.count {
            let stored = self.values[index].to_le_bytes();
            for (byte, destination) in bytes[..width as usize].iter_mut().enumerate() {
                let offset = address
                    .wrapping_add(byte as u32)
                    .wrapping_sub(self.addresses[index]);
                if offset < width {
                    *destination = stored[offset as usize];
                }
            }
        }
        Ok(u32::from_le_bytes(bytes))
    }
}

// the synchronous batch profile checks all cached ss authority before probing ram.
// this does not define a universal priority between independently faulting operands.
fn check_segments(
    context: &SegmentContext,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    original: [u32; 8],
) -> Result<(), CpuMemoryError> {
    let width = kind.width();
    let span = |offset, access| {
        super::segments::check_cached_span(context, SegmentRegister::Ss, offset, width, access)
    };
    match kind {
        StackFrameKind::PushAll32 | StackFrameKind::PushAll16 => {
            for index in 1..=8 {
                span(
                    original[4].wrapping_sub(index * width),
                    SegmentAccess::Write,
                )?;
            }
        }
        StackFrameKind::PopAll32 | StackFrameKind::PopAll16 => {
            for slot in [0, 1, 2, 4, 5, 6, 7] {
                span(original[4].wrapping_add(slot * width), SegmentAccess::Read)?;
            }
        }
        StackFrameKind::Enter32 | StackFrameKind::Enter16 => {
            span(original[4].wrapping_sub(width), SegmentAccess::Write)?;
            for index in 1..u32::from(nesting) {
                span(original[5].wrapping_sub(index * width), SegmentAccess::Read)?;
                span(
                    original[4].wrapping_sub((index + 1) * width),
                    SegmentAccess::Write,
                )?;
            }
            let count = if nesting == 0 {
                1
            } else {
                u32::from(nesting) + 1
            };
            if nesting != 0 {
                span(
                    original[4].wrapping_sub(count * width),
                    SegmentAccess::Write,
                )?;
            }
            span(
                original[4].wrapping_sub(count * width + u32::from(allocation)),
                SegmentAccess::Write,
            )?;
        }
    }
    Ok(())
}

fn prepare(
    memory: &AddressSpace,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    original: [u32; 8],
) -> Result<Prepared, MemoryError> {
    let width = kind.width();
    let mut result = Prepared {
        registers: original,
        addresses: [0; 32],
        values: [0; 32],
        count: 0,
    };
    match kind {
        StackFrameKind::PushAll32 | StackFrameKind::PushAll16 => {
            for register in [0, 1, 2, 3, 4, 5, 6, 7] {
                result.push(memory, width, original[register])?;
            }
        }
        StackFrameKind::PopAll32 | StackFrameKind::PopAll16 => {
            for (slot, register) in [(0, 7), (1, 6), (2, 5), (4, 3), (5, 2), (6, 1), (7, 0)] {
                let value = result.read(memory, original[4].wrapping_add(slot * width), width)?;
                result.registers[register] = if width == 2 {
                    (original[register] & !0xffff) | value
                } else {
                    value
                };
            }
            result.registers[4] = original[4].wrapping_add(8 * width);
        }
        StackFrameKind::Enter32 | StackFrameKind::Enter16 => {
            result.push(memory, width, original[5])?;
            let frame = result.registers[4];
            for index in 1..u32::from(nesting) {
                let value = result.read(memory, original[5].wrapping_sub(index * width), width)?;
                result.push(memory, width, value)?;
            }
            if nesting != 0 {
                result.push(memory, width, frame)?;
            }
            result.registers[5] = if width == 2 {
                (original[5] & !0xffff) | (frame & 0xffff)
            } else {
                frame
            };
            // the software profile fixes SS.B=1: all stack addresses and allocation use ESP.
            result.registers[4] = result.registers[4].wrapping_sub(u32::from(allocation));
            // intel ENTER requires a writeability check at the final stack pointer,
            // including local storage which is allocated without being initialized.
            memory.check_span(
                GuestAddress(result.registers[4]),
                width as usize,
                Access::Write,
            )?;
        }
    }
    Ok(result)
}

impl EngineInstance {
    pub fn stack_execute(&mut self, control: u32) -> Result<StoreCompletion, HostError> {
        self.guard(self.key, self.generation())?;
        self.stack_execute_bound(None, control)
    }

    pub fn stack_execute_resident(
        &mut self,
        key: u64,
        id: u64,
        control: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_resident(key, id)?;
        self.stack_execute_bound(Some(id), control)
    }

    fn stack_execute_bound(
        &mut self,
        resident: Option<u64>,
        control: u32,
    ) -> Result<StoreCompletion, HostError> {
        let (kind, allocation, nesting) =
            StackFrameKind::decode(control).ok_or(HostError::InvalidRequest)?;
        let input = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
        let original = std::array::from_fn(|index| {
            u32::from_le_bytes(input[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let context = self.segment_context()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let result = check_segments(&context, kind, allocation, nesting, original)
            .and_then(|()| {
                prepare(memory, kind, allocation, nesting, original).map_err(CpuMemoryError::Memory)
            })
            .and_then(|prepared| {
                memory
                    .write_stack(
                        &prepared.addresses[..prepared.count],
                        &prepared.values[..prepared.count],
                        kind.width() as usize,
                    )
                    .map_err(CpuMemoryError::Memory)?;
                Ok(prepared.registers)
            });
        let changed = result.is_ok() && version != memory.code_validation_version();
        let (length, packet) = match result {
            Ok(registers) => {
                for (index, value) in registers.into_iter().enumerate() {
                    self.arena.as_mut().get_mut()
                        [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                        .copy_from_slice(&value.to_le_bytes());
                }
                (0, Ok(()))
            }
            Err(error) => (
                if matches!(
                    error,
                    CpuMemoryError::Memory(MemoryError::Fault(_)) | CpuMemoryError::StackSegment(_)
                ) {
                    kind.width()
                } else {
                    0
                },
                Err(error),
            ),
        };
        encode_cpu_result(
            length,
            0,
            packet,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        self.cpu_store_currentness(changed, resident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    fn memory() -> AddressSpace {
        let mut memory = AddressSpace::new(3).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x7000), 2).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
    }

    #[test]
    fn push_all_preserves_original_sp_and_pop_all_skips_that_slot() {
        for kind in [StackFrameKind::PushAll16, StackFrameKind::PushAll32] {
            let mut memory = memory();
            let registers = [
                0x11112222, 0x22223333, 0x33334444, 0x44445555, 0x8080, 0x55556666, 0x66667777,
                0x77778888,
            ];
            let pushed = prepare(&memory, kind, 0, 0, registers).unwrap();
            memory
                .write_stack(
                    &pushed.addresses[..8],
                    &pushed.values[..8],
                    kind.width() as usize,
                )
                .unwrap();
            assert_eq!(pushed.values[4], registers[4]);
            let popped = prepare(
                &memory,
                if kind.width() == 2 {
                    StackFrameKind::PopAll16
                } else {
                    StackFrameKind::PopAll32
                },
                0,
                0,
                pushed.registers,
            )
            .unwrap();
            assert_eq!(popped.registers, registers);
        }
    }

    #[test]
    fn nested_enter_reads_prior_push_when_the_frame_overlaps_and_does_not_clear_locals() {
        let mut memory = memory();
        memory.write(GuestAddress(0x8000), &[0xa5; 128]).unwrap();
        let registers = [0, 0, 0, 0, 0x8080, 0x8080, 0, 0];
        let prepared = prepare(&memory, StackFrameKind::Enter32, 16, 3, registers).unwrap();
        assert_eq!(&prepared.values[..4], &[0x8080, 0x8080, 0x8080, 0x807c]);
        assert_eq!(
            (prepared.registers[4], prepared.registers[5]),
            (0x8060, 0x807c)
        );
        memory
            .write_stack(&prepared.addresses[..4], &prepared.values[..4], 4)
            .unwrap();
        let mut locals = [0; 16];
        memory.read(GuestAddress(0x8060), &mut locals).unwrap();
        assert_eq!(locals, [0xa5; 16]);
    }

    #[test]
    fn final_allocation_probe_and_late_frame_read_fail_before_any_commit() {
        let memory = memory();
        let registers = [0, 0, 0, 0, 0x8004, 0x9004, 0, 0];
        assert!(prepare(&memory, StackFrameKind::Enter32, 0, 2, registers).is_err());
        let mut registers = registers;
        registers[5] = 0x8000;
        assert!(prepare(&memory, StackFrameKind::Enter32, 0x1001, 0, registers).is_err());
        let mut bytes = [1; 4];
        memory.read(GuestAddress(0x8000), &mut bytes).unwrap();
        assert_eq!(bytes, [0; 4]);
    }
}
