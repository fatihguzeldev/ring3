use super::segment_support::{Context, REGISTERS, Ram};
use ring3_engine::cpu::x86::StackFrameKind;

fn push(registers: [u32; 8], width: u32, ram: &mut Ram) -> [u32; 8] {
    for (index, value) in registers.into_iter().enumerate() {
        let address = registers[4].wrapping_sub((index as u32 + 1) * width);
        for byte in 0..width {
            ram.insert(address + byte, (value >> (byte * 8)) as u8);
        }
    }
    let mut result = registers;
    result[4] = result[4].wrapping_sub(8 * width);
    result
}

fn pop(registers: [u32; 8], width: u32, ram: &Ram) -> [u32; 8] {
    let mut result = registers;
    for (slot, register) in [(0, 7), (1, 6), (2, 5), (4, 3), (5, 2), (6, 1), (7, 0)] {
        let address = registers[4].wrapping_add(slot * width);
        let value = (0..width).fold(0, |value, byte| {
            value | (u32::from(ram[&(address + byte)]) << (8 * byte))
        });
        result[register] = if width == 2 {
            (registers[register] & 0xffff_0000) | value
        } else {
            value
        };
    }
    result[4] = result[4].wrapping_add(8 * width);
    result
}

#[test]
fn every_push_slot_checks_cached_ss_before_any_ram_or_staged_publication() {
    for resident in [false, true] {
        for entries in [false, true] {
            for kind in [StackFrameKind::PushAll16, StackFrameKind::PushAll32] {
                let width = kind.width();
                let mut context = Context::new(resident, entries);
                let boundary = REGISTERS[4] - 8 * width;
                context.seed(REGISTERS, Some(0xf270_0000 | boundary));
                context.failure(kind, 0, 0, [7, 0, 0, 0, 0, width], None);
                context.seed(REGISTERS, Some(0xf270_0000 | (boundary - 1)));
                let mut ram = context.ram();
                let result = push(REGISTERS, width, &mut ram);
                context.success(kind, 0, 0, result, &ram);
            }
        }
    }
}

#[test]
fn pop_checks_all_seven_full_read_spans_and_preserves_word_upper_halves() {
    for resident in [false, true] {
        for entries in [false, true] {
            for kind in [StackFrameKind::PopAll16, StackFrameKind::PopAll32] {
                let width = kind.width();
                let mut context = Context::new(resident, entries);
                let mut registers = REGISTERS;
                registers[4] = 0x8020;
                let end = registers[4] + 8 * width - 1;
                context.seed(registers, Some(0xf230_0000 | (end - 1)));
                context.failure(kind, 0, 0, [7, 0, 0, 0, 0, width], None);
                context.seed(registers, Some(0xf230_0000 | end));
                let ram = context.ram();
                context.success(kind, 0, 0, pop(registers, width, &ram), &ram);
            }
        }
    }
}

#[test]
fn stack_slots_may_wrap_between_operands_but_each_operand_span_must_fit() {
    for resident in [false, true] {
        for (push_kind, pop_kind) in [
            (StackFrameKind::PushAll16, StackFrameKind::PopAll16),
            (StackFrameKind::PushAll32, StackFrameKind::PopAll32),
        ] {
            let width = push_kind.width();
            let mut context = Context::new(resident, false);
            let mut registers = REGISTERS;
            registers[4] = 4 * width;
            context.seed(registers, None);
            let mut ram = context.ram();
            let pushed = push(registers, width, &mut ram);
            context.success(push_kind, 0, 0, pushed, &ram);
            // a poisoned saved ESP operand must never restore the stack pointer.
            let skipped = pushed[4].wrapping_add(3 * width);
            context.engine.arena_mut().unwrap()[140..140 + width as usize].fill(0xff);
            context.engine.upload(skipped, width).unwrap();
            context.seed(pushed, None);
            let ram = context.ram();
            context.success(pop_kind, 0, 0, pop(pushed, width, &ram), &ram);
            for (kind, sp) in [(push_kind, 1), (pop_kind, 1_u32.wrapping_sub(width))] {
                registers[4] = sp;
                context.seed(registers, None);
                context.failure(kind, 0, 0, [7, 0, 0, 0, 0, width], None);
            }
        }
    }
}

#[test]
fn page_failures_remain_atomic_and_cached_ss_preflight_precedes_ram_by_profile() {
    for resident in [false, true] {
        for (push_kind, pop_kind) in [
            (StackFrameKind::PushAll16, StackFrameKind::PopAll16),
            (StackFrameKind::PushAll32, StackFrameKind::PopAll32),
        ] {
            let width = push_kind.width();
            let mut context = Context::new(resident, false);
            let mut registers = REGISTERS;
            registers[4] = 0x8000 + 2 * width;
            context.seed(registers, None);
            context.failure(
                push_kind,
                0,
                0,
                [1, 0, 2, 0x8000 - width, 2, width],
                Some((0x7000, 1)),
            );
            context.seed(registers, Some(0xf270_0000 | (registers[4] - 8 * width)));
            context.failure(push_kind, 0, 0, [7, 0, 0, 0, 0, width], Some((0x8000, 1)));
            registers[4] = 0x8000 - 2 * width;
            context.seed(registers, Some(0xf33f_ffff));
            context.failure(
                pop_kind,
                0,
                0,
                [1, 0, 2, 0x8000, 1, width],
                Some((0x8000, 2)),
            );
        }
    }
}
