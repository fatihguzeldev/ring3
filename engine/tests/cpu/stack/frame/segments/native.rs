use super::segment_support::{Context, REGISTERS, Ram};
use ring3_engine::cpu::x86::StackFrameKind;

fn enter(registers: [u32; 8], width: u32, allocation: u16, nesting: u8, ram: &mut Ram) -> [u32; 8] {
    let mut result = registers;
    let level = u32::from(nesting & 31);
    let frame = registers[4].wrapping_sub(width);
    let mut values = vec![registers[5]];
    for index in 1..level {
        // materialize each already-staged push before reading an overlapping old frame.
        let previous = values.len() - 1;
        let address = registers[4].wrapping_sub((previous as u32 + 1) * width);
        for byte in 0..width {
            ram.insert(address + byte, (values[previous] >> (byte * 8)) as u8);
        }
        let source = registers[5].wrapping_sub(index * width);
        values.push((0..width).fold(0, |value, byte| {
            value | (u32::from(ram[&(source + byte)]) << (8 * byte))
        }));
    }
    if level != 0 {
        values.push(frame);
    }
    for (index, value) in values.iter().enumerate() {
        let address = registers[4].wrapping_sub((index as u32 + 1) * width);
        for byte in 0..width {
            ram.insert(address + byte, (value >> (byte * 8)) as u8);
        }
    }
    result[4] = registers[4]
        .wrapping_sub(values.len() as u32 * width)
        .wrapping_sub(u32::from(allocation));
    result[5] = if width == 2 {
        (registers[5] & 0xffff_0000) | (frame & 0xffff)
    } else {
        frame
    };
    result
}

#[test]
fn enter_nested_overlay_and_allocated_local_bytes_keep_the_existing_transaction() {
    for resident in [false, true] {
        for entries in [false, true] {
            for kind in [StackFrameKind::Enter16, StackFrameKind::Enter32] {
                for nesting in [0, 1, 3, 31, 35] {
                    let mut context = Context::new(resident, entries);
                    context.seed(REGISTERS, None);
                    let mut ram = context.ram();
                    let expected = enter(REGISTERS, kind.width(), 16, nesting, &mut ram);
                    context.success(kind, 16, nesting, expected, &ram);
                }
            }
        }
    }
}

#[test]
fn final_allocation_uses_the_same_logical_offset_and_full_width_as_ram_probe() {
    for resident in [false, true] {
        for entries in [false, true] {
            for kind in [StackFrameKind::Enter16, StackFrameKind::Enter32] {
                for nesting in [0, 1, 3] {
                    let width = kind.width();
                    let count = if nesting == 0 {
                        1
                    } else {
                        u32::from(nesting) + 1
                    };
                    let boundary = REGISTERS[4] - count * width - 16;
                    let mut context = Context::new(resident, entries);
                    context.seed(REGISTERS, Some(0xf270_0000 | boundary));
                    context.failure(kind, 16, nesting, [7, 0, 0, 0, 0, width], None);
                    context.seed(REGISTERS, Some(0xf270_0000 | (boundary - 1)));
                    let mut ram = context.ram();
                    let expected = enter(REGISTERS, width, 16, nesting, &mut ram);
                    context.success(kind, 16, nesting, expected, &ram);
                }
            }
        }
    }
}

#[test]
fn nested_reads_use_original_full_ebp_and_every_late_push_checks_cached_ss() {
    for resident in [false, true] {
        for kind in [StackFrameKind::Enter16, StackFrameKind::Enter32] {
            let width = kind.width();
            let mut context = Context::new(resident, false);
            let mut registers = REGISTERS;
            registers[5] = 0x10000 + width;
            context.seed(registers, Some(0xf230_8fff));
            context.failure(kind, 0, 3, [7, 0, 0, 0, 0, width], None);
            context.seed(REGISTERS, Some(0xf270_0000 | (REGISTERS[4] - 4 * width)));
            context.failure(kind, 0, 3, [7, 0, 0, 0, 0, width], None);
            registers = REGISTERS;
            registers[4] = 0;
            registers[5] = 0;
            context.seed(registers, Some(0xf33f_ffff));
            let mut ram = context.ram();
            let expected = enter(registers, width, 0, 3, &mut ram);
            context.success(kind, 0, 3, expected, &ram);
            registers = REGISTERS;
            registers[4] = 0x10008;
            registers[5] = 0xaabb_8080;
            context.seed(registers, Some(0xf33f_ffff));
            let mut ram = context.ram();
            let expected = enter(registers, width, 16, 0, &mut ram);
            context.success(kind, 16, 0, expected, &ram);
            registers[4] = width;
            context.seed(registers, None);
            context.failure(kind, 1, 0, [7, 0, 0, 0, 0, width], None);
        }
    }
}

#[test]
fn late_frame_reads_and_final_allocation_page_failures_do_not_publish_earlier_pushes() {
    for resident in [false, true] {
        for kind in [StackFrameKind::Enter16, StackFrameKind::Enter32] {
            let width = kind.width();
            let mut context = Context::new(resident, false);
            let mut registers = REGISTERS;
            registers[5] = 0x6000 + width;
            context.seed(registers, None);
            context.failure(kind, 0, 3, [1, 0, 2, 0x6000, 1, width], Some((0x6000, 2)));
            context.seed(REGISTERS, None);
            context.failure(
                kind,
                0x1000,
                0,
                [1, 0, 2, 0x7080 - width, 2, width],
                Some((0x7000, 1)),
            );
            // this is Ring3's all-segment-before-RAM policy, not a universal fault ordering claim.
            context.seed(REGISTERS, Some(0xf270_0000 | (REGISTERS[4] - width - 16)));
            context.failure(kind, 16, 0, [7, 0, 0, 0, 0, width], Some((0x8000, 1)));
        }
    }
}
