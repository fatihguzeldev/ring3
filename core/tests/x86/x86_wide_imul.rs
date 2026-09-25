use super::executable;
use super::wide_product_executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

fn encoding(bits: u32, source: u8, address: Option<u32>) -> Vec<u8> {
    let mut code = Vec::new();
    if bits == 16 {
        code.push(0x66);
    }
    code.extend([if bits == 8 { 0xf6 } else { 0xf7 }, 0x28 | source]);
    if let Some(address) = address {
        code.extend(address.to_le_bytes());
    }
    code
}

#[test]
fn full_product_forms_run_whole_or_stepwise() {
    for budget in [1, 40] {
        let image = load_pe32(&wide_product_executable::pe32(), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        let mut count = 0;
        loop {
            let run = cpu.run(&mut memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 40);
        }
        assert_eq!(count, 10);
        assert_eq!(cpu.register(Register32::Esi), u32::MAX);
        assert_eq!(cpu.register(Register32::Edi), u32::MAX);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_fffa);
        assert_eq!(cpu.eflags & 0x801, 0);
    }
}

#[test]
fn all_widths_match_wide_integer_products_and_signed_representability() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let low_mask = if bits == 8 { 0xffff } else { mask };
        let minimum = -(1_i128 << (bits - 1));
        let maximum = (1_i128 << (bits - 1)) - 1;
        let values = [minimum, maximum, -7, -1, 0, 1, 7];
        for left in values {
            for right in values {
                for source_memory in [false, true] {
                    let code = encoding(
                        bits,
                        if source_memory { 5 } else { 0xc3 },
                        source_memory.then_some(0x0040_2ffc),
                    );
                    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
                    let left_bits = u32::try_from(left & i128::from(mask)).unwrap();
                    let right_bits = u32::try_from(right & i128::from(mask)).unwrap();
                    image
                        .memory
                        .write(0x0040_2ffc, &right_bits.to_le_bytes())
                        .unwrap();
                    image
                        .memory
                        .protect(0x0040_2000, 4096, Permissions::READ)
                        .unwrap();
                    let mut cpu = Cpu32::new(image.entry_point);
                    cpu.set_register(Register32::Eax, (0xabcd_1234 & !mask) | left_bits);
                    cpu.set_register(Register32::Ebx, (0x1234_5678 & !mask) | right_bits);
                    cpu.set_register(Register32::Edx, 0x5678_9abc);
                    cpu.eflags = if source_memory {
                        0xced7
                    } else {
                        0xced7 & !0x801
                    };
                    let product = left * right;
                    let mut expected = cpu;
                    expected.eip += u32::try_from(code.len()).unwrap();
                    expected.set_register(
                        Register32::Eax,
                        (cpu.register(Register32::Eax) & !low_mask)
                            | u32::try_from(product & i128::from(low_mask)).unwrap(),
                    );
                    if bits != 8 {
                        expected.set_register(
                            Register32::Edx,
                            (cpu.register(Register32::Edx) & !mask)
                                | u32::try_from((product >> bits) & i128::from(mask)).unwrap(),
                        );
                    }
                    expected.eflags = (cpu.eflags & !0x801)
                        | if (minimum..=maximum).contains(&product) {
                            0
                        } else {
                            0x801
                        };
                    assert_eq!(
                        cpu.run(&mut image.memory, 1).reason,
                        StopReason::InstructionLimit
                    );
                    assert_eq!(cpu, expected, "{bits}/{left}/{right}/{source_memory}");
                    let mut bytes = [0; 4];
                    image.memory.read(0x0040_2ffc, &mut bytes).unwrap();
                    assert_eq!(bytes, right_bits.to_le_bytes());
                }
            }
        }
    }
}

#[test]
fn accumulator_high_byte_and_high_result_sources_are_captured_before_writes() {
    for (bits, source, eax, edx, low, high) in [
        (8, 0xc4, 0xabcd_03fe, 0x1234_5678, 0xabcd_fffa, 0x1234_5678),
        (8, 0xc0, 0xabcd_ef80, 0x1234_5678, 0xabcd_4000, 0x1234_5678),
        (16, 0xc2, 0xabcd_fffe, 0x1234_0003, 0xabcd_fffa, 0x1234_ffff),
        (16, 0xc0, 0xabcd_8000, 0x1234_5678, 0xabcd_0000, 0x1234_4000),
        (32, 0xc2, 0xffff_fffe, 3, 0xffff_fffa, u32::MAX),
        (32, 0xc0, 0x8000_0000, 0x1234_5678, 0, 0x4000_0000),
    ] {
        let code = encoding(bits, source, None);
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Edx, edx);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax), low);
        assert_eq!(cpu.register(Register32::Edx), high);
    }
}

#[test]
fn source_faults_prefix_refusals_and_zero_budget_preserve_complete_cpu() {
    for bits in [8, 16, 32] {
        let invalid = if bits == 8 { 0x0040_3000 } else { u32::MAX };
        for address in [0x6000_0000, invalid] {
            let code = encoding(bits, 5, Some(address));
            let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, 7);
            cpu.set_register(Register32::Edx, 99);
            cpu.eflags = 0xced7;
            let before = cpu;
            assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
            assert_eq!(cpu, before);
            assert!(matches!(
                cpu.run(&mut image.memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
    }
    for prefix in [0xf3, 0xf2, 0x67] {
        let code = [prefix, 0xf7, 0x2f];
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let mut image = load_pe32(&executable::pe32(&[0xf0, 0xf7, 0xeb]), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::InvalidInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn fs_wrap_last_guest_bytes_and_split_page_retry_are_checked_before_mutation() {
    for bits in [8, 16, 32] {
        let width = bits / 8;
        let mut code = vec![0x64];
        code.extend(encoding(bits, 5, Some(0xffff_fff0)));
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x0040_2ff0 + 16);
        cpu.set_register(Register32::Eax, 7);
        image
            .memory
            .write(0x0040_2ff0, &[0xfe, 0xff, 0xff, 0xff])
            .unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax) & 0xffff, 0xfff2);
        let address = u32::MAX - width + 1;
        let code = encoding(bits, 5, Some(address));
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        image
            .memory
            .write(
                u64::from(address),
                &vec![0xff; usize::try_from(width).unwrap()],
            )
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 7);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax) & 0xffff, 0xfff9);
    }
    let code = encoding(32, 5, Some(0x0040_2fff));
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 7);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    image
        .memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    image
        .memory
        .write(0x0040_2fff, &(-3_i32).to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), (-21_i32).cast_unsigned());
    assert_eq!(cpu.register(Register32::Edx), u32::MAX);
}
