use crate::process::{EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::{
        CpuMemoryPort32, CpuPagingError32, CpuReadPort32, PagingAccess32, SegmentAccess,
        SegmentContext, SegmentRegister, StackFrameKind, StackPointerSize,
    },
    memory::{GuestAddress, MemoryError},
};

pub(in crate::process) struct Prepared {
    ss_base: u32,
    pointer: StackPointerSize,
    pub(in crate::process) registers: [u32; 8],
    pub(in crate::process) addresses: [u32; 32],
    pub(in crate::process) values: [u32; 32],
    pub(in crate::process) count: usize,
}

impl Prepared {
    fn push(
        &mut self,
        memory: &mut impl CpuReadPort32,
        width: u32,
        value: u32,
    ) -> Result<(), CpuPagingError32> {
        let advanced = self.pointer.advance(self.registers[4], -(width as i32));
        let address = self.ss_base.wrapping_add(self.pointer.offset(advanced));
        memory.check(GuestAddress(address), width as usize, PagingAccess32::Write)?;
        self.addresses[self.count] = address;
        self.values[self.count] = value;
        self.count += 1;
        self.registers[4] = advanced;
        Ok(())
    }

    fn read(
        &self,
        memory: &mut impl CpuReadPort32,
        offset: u32,
        width: u32,
    ) -> Result<u32, CpuPagingError32> {
        let address = self.ss_base.wrapping_add(self.pointer.offset(offset));
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
pub(in crate::process) fn check_segments(
    context: &SegmentContext,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    original: [u32; 8],
) -> Result<(), CpuMemoryError> {
    let width = kind.width();
    let pointer = StackPointerSize::new(context.caches[SegmentRegister::Ss.index()].default32());
    let span = |offset, access| {
        super::segments::check_cached_span(
            context,
            SegmentRegister::Ss,
            pointer.offset(offset),
            width,
            access,
        )
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

pub(in crate::process) fn prepare(
    memory: &mut impl CpuReadPort32,
    ss_base: u32,
    pointer: StackPointerSize,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    original: [u32; 8],
) -> Result<Prepared, CpuPagingError32> {
    let width = kind.width();
    let mut result = Prepared {
        ss_base,
        pointer,
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
            result.registers[4] = pointer.advance(original[4], (8 * width) as i32);
        }
        StackFrameKind::Enter32 | StackFrameKind::Enter16 => {
            result.push(memory, width, original[5])?;
            // the frame value follows operand width; only implicit accesses use the active pointer.
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
            result.registers[4] = pointer.advance(result.registers[4], -i32::from(allocation));
            // intel ENTER requires a writeability check at the final stack pointer,
            // including local storage which is allocated without being initialized.
            memory.check(
                GuestAddress(ss_base.wrapping_add(pointer.offset(result.registers[4]))),
                width as usize,
                PagingAccess32::Write,
            )?;
        }
    }
    Ok(result)
}

impl EngineInstance {
    pub fn stack_execute(&mut self, control: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.stack_execute_bound(None, control)
    }

    pub fn stack_execute_resident(
        &mut self,
        key: u64,
        id: u64,
        control: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
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
        let context = self.operand_segment_context()?;
        let pointer =
            StackPointerSize::new(context.caches[SegmentRegister::Ss.index()].default32());
        let version = self.memory()?.code_validation_version();
        let mut memory = self.cpu_memory_port32();
        let result = check_segments(&context, kind, allocation, nesting, original)
            .and_then(|()| {
                prepare(
                    &mut memory,
                    context.state.bases[SegmentRegister::Ss.index()],
                    pointer,
                    kind,
                    allocation,
                    nesting,
                    original,
                )
                .map_err(CpuMemoryError::from)
            })
            .and_then(|prepared| {
                memory
                    .write_stack(
                        &prepared.addresses[..prepared.count],
                        &prepared.values[..prepared.count],
                        kind.width() as usize,
                    )
                    .map_err(CpuMemoryError::from)?;
                Ok(prepared.registers)
            });
        let changed = result.is_ok() && version != self.memory()?.code_validation_version();
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
                    CpuMemoryError::Memory(MemoryError::Fault(_))
                        | CpuMemoryError::Paging(_)
                        | CpuMemoryError::StackSegment(_)
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
    use crate::{
        cpu::x86::DefaultCpuReadPort32,
        memory::{AddressSpace, PageRange, Permissions},
    };

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
            let pushed = prepare(
                &mut DefaultCpuReadPort32::new(&memory),
                0,
                StackPointerSize::new(true),
                kind,
                0,
                0,
                registers,
            )
            .unwrap();
            memory
                .write_stack(
                    &pushed.addresses[..8],
                    &pushed.values[..8],
                    kind.width() as usize,
                )
                .unwrap();
            assert_eq!(pushed.values[4], registers[4]);
            let popped = prepare(
                &mut DefaultCpuReadPort32::new(&memory),
                0,
                StackPointerSize::new(true),
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
        let prepared = prepare(
            &mut DefaultCpuReadPort32::new(&memory),
            0,
            StackPointerSize::new(true),
            StackFrameKind::Enter32,
            16,
            3,
            registers,
        )
        .unwrap();
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
        assert!(
            prepare(
                &mut DefaultCpuReadPort32::new(&memory),
                0,
                StackPointerSize::new(true),
                StackFrameKind::Enter32,
                0,
                2,
                registers
            )
            .is_err()
        );
        let mut registers = registers;
        registers[5] = 0x8000;
        assert!(
            prepare(
                &mut DefaultCpuReadPort32::new(&memory),
                0,
                StackPointerSize::new(true),
                StackFrameKind::Enter32,
                0x1001,
                0,
                registers
            )
            .is_err()
        );
        let mut bytes = [1; 4];
        memory.read(GuestAddress(0x8000), &mut bytes).unwrap();
        assert_eq!(bytes, [0; 4]);
    }

    #[test]
    fn nonflat_enter_overlay_and_batch_registers_stay_logical() {
        let mut memory = memory();
        memory.write(GuestAddress(0x8000), &[0xa5; 128]).unwrap();
        let registers = [0, 0, 0, 0, 0x3080, 0x3080, 0, 0];
        let prepared = prepare(
            &mut DefaultCpuReadPort32::new(&memory),
            0x5000,
            StackPointerSize::new(true),
            StackFrameKind::Enter32,
            16,
            3,
            registers,
        )
        .unwrap();
        assert_eq!(&prepared.addresses[..4], &[0x807c, 0x8078, 0x8074, 0x8070]);
        assert_eq!(&prepared.values[..4], &[0x3080, 0x3080, 0x3080, 0x307c]);
        assert_eq!(
            (prepared.registers[4], prepared.registers[5]),
            (0x3060, 0x307c)
        );
    }
    #[test]
    fn nonflat_late_stack_page_fault_keeps_ram_registers_and_code_version_atomic() {
        for kind in [StackFrameKind::PushAll32, StackFrameKind::PushAll16] {
            let mut engine = EngineInstance::new(2, 91).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.map(0x8000, 1, 3).unwrap();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
                .copy_from_slice(&[0xeb, 0xfe]);
            engine.upload(0x1000, 2).unwrap();
            for (index, value) in [0x1000_u32, 2].into_iter().enumerate() {
                engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            engine.compile(1).unwrap();
            engine
                .set_segment_base(SegmentRegister::Ss, 0x5000)
                .unwrap();
            let width = kind.width();
            let original = [1_u32, 2, 3, 4, 0x3000 + width, 6, 7, 8];
            for (index, value) in original.into_iter().enumerate() {
                engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            let before = engine.arena().to_vec();
            let version = engine.memory().unwrap().code_validation_version();
            let mut before_ram = [0; 16];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x8000), &mut before_ram)
                .unwrap();
            assert_eq!(
                engine.stack_execute(kind.control(0, 0)),
                Ok(StoreCompletion::Complete)
            );
            let mut packet = [0; HELPER_SIZE];
            packet[..16].copy_from_slice(&[82, 51, 77, 72, 15, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
            for (index, value) in [9_u32, 0, 6, 0x8000 - width, 2, width]
                .into_iter()
                .enumerate()
            {
                packet[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
            }
            assert_eq!(engine.cr2(), Some(GuestAddress(0x8000 - width)));
            let mut expected = before;
            expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
            assert_eq!(engine.arena(), expected);
            let mut after_ram = [0; 16];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x8000), &mut after_ram)
                .unwrap();
            assert_eq!(after_ram, before_ram);
            assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        }
    }
    #[test]
    fn mixed_enter32_stages_full_frame_value_and_active_bp_overlay() {
        let memory = memory();
        let original = [1, 2, 3, 4, 0xabcd3080, 0x76543080, 7, 8];
        let prepared = prepare(
            &mut DefaultCpuReadPort32::new(&memory),
            0x5000,
            StackPointerSize::new(false),
            StackFrameKind::Enter32,
            16,
            3,
            original,
        )
        .unwrap();
        assert_eq!(&prepared.addresses[..4], &[0x807c, 0x8078, 0x8074, 0x8070]);
        assert_eq!(
            &prepared.values[..4],
            &[0x76543080, 0x76543080, 0x76543080, 0xabcd307c]
        );
        assert_eq!(
            (prepared.registers[4], prepared.registers[5]),
            (0xabcd3060, 0xabcd307c)
        );
    }

    #[test]
    fn mixed_enter32_final_probe_failure_preserves_code_validation_version() {
        use crate::abi::x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context,
        };
        let mut engine = EngineInstance::new(2, 0x53425353).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x6000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        let mut context = engine.segment_context().unwrap();
        context.state.bases[2] = 0x5000;
        context.caches[2] =
            crate::cpu::x86::SegmentCache::from_bits(context.caches[2].bits() & !0x02000000)
                .unwrap();
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        engine.arena_mut().unwrap()[crate::abi::arena::SEGMENTS_OFFSET
            ..crate::abi::arena::SEGMENTS_OFFSET + SEGMENTS_SIZE]
            .copy_from_slice(&segments);
        engine.arena_mut().unwrap()[crate::abi::arena::SEGMENT_BASES_RECORD_OFFSET
            ..crate::abi::arena::SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(&bases);
        for (index, value) in [1_u32, 2, 3, 4, 0xabcd1004, 0x76540024, 7, 8]
            .into_iter()
            .enumerate()
        {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        let mut before_ram = [0; 16];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x6000), &mut before_ram)
            .unwrap();
        assert_eq!(
            engine.stack_execute(StackFrameKind::Enter32.control(8, 0)),
            Ok(StoreCompletion::Complete)
        );
        let mut packet = [0; HELPER_SIZE];
        packet[..16].copy_from_slice(&[82, 51, 77, 72, 15, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
        for (index, value) in [9_u32, 0, 6, 0x5ff8, 2, 4].into_iter().enumerate() {
            packet[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(engine.cr2(), Some(GuestAddress(0x5ff8)));
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
        assert_eq!(engine.arena(), expected);
        let mut after_ram = [0; 16];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x6000), &mut after_ram)
            .unwrap();
        assert_eq!(after_ram, before_ram);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[cfg(test)]
#[path = "stack/linear_wrap.rs"]
mod linear_wrap;
