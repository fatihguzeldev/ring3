use super::icon_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const LOAD: u32 = 0x7000_02c8;
const STACK: u32 = 0x1000_ff00;

fn prepare(p: &mut Process32, module: u32, name: u32) {
    p.cpu.eip = LOAD;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.memory
        .write(
            u64::from(STACK),
            &[0x0040_1000_u32, module, name]
                .map(u32::to_le_bytes)
                .concat(),
        )
        .unwrap();
}

fn call(p: &mut Process32, module: u32, name: u32) -> u32 {
    prepare(p, module, name);
    let mut expected = p.cpu;
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.set_register(Register32::Eax, value);
    expected.set_register(Register32::Esp, STACK + 12);
    expected.eip = 0x0040_1000;
    assert_eq!(p.cpu, expected);
    value
}

fn stopped(p: &mut Process32) -> ProcessStop {
    let cpu = p.cpu;
    let result = p.run(1);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    result.reason
}

#[test]
fn imported_icon_load_reuses_the_selected_resource_across_budgets() {
    let mut expected = None;
    for budget in [1, 100] {
        let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(p.cpu.register(Register32::Eax), 0x7800_0004);
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7800_0004);
        if let Some(previous) = expected {
            assert_eq!((p.cpu, counts), previous);
        }
        expected = Some((p.cpu, counts));
    }
}

#[test]
fn cached_icons_do_not_reread_payload_or_touch_error_fields() {
    let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
    let pages = p.memory.mapped_pages();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
    p.memory
        .protect(0x0040_2000, 8192, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn missing_and_unsupported_requests_do_not_publish_handles() {
    let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
    for (module, name, error) in [(123, 7, 87), (0x7000_0800, 7, 1812), (0x0040_0000, 8, 1814)] {
        assert_eq!(call(&mut p, module, name), 0);
        assert_eq!(p.last_error().unwrap(), error);
    }
    for (module, name) in [(0, 32512), (0x0040_0000, 0x0040_2180)] {
        prepare(&mut p, module, name);
        assert_eq!(
            stopped(&mut p),
            ProcessStop::UnsupportedApi { address: LOAD }
        );
    }
    p.memory
        .write(icon_executable::GROUP + 32, &3_u16.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0);
    assert_eq!(p.last_error().unwrap(), 1814);
    p.memory
        .write(icon_executable::GROUP + 32, &2_u16.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
}

#[test]
fn selected_image_is_validated_without_falling_back_to_another_depth() {
    for (address, value) in [
        (icon_executable::GROUP, 2_u32),
        (icon_executable::GROUP + 28, 1),
        (icon_executable::SECOND_IMAGE, 108),
        (icon_executable::SECOND_IMAGE + 8, 63),
        (icon_executable::SECOND_IMAGE + 16, 1),
    ] {
        let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
        let mut original = [0; 4];
        p.memory.read(address, &mut original).unwrap();
        p.memory.write(address, &value.to_le_bytes()).unwrap();
        prepare(&mut p, 0x0040_0000, 7);
        assert_eq!(
            stopped(&mut p),
            ProcessStop::UnsupportedApi { address: LOAD }
        );
        p.memory.write(address, &original).unwrap();
        assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
    }
    let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
    p.memory
        .write(icon_executable::FIRST_IMAGE, &108_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
}

#[test]
fn resource_and_error_page_faults_allow_retry_without_losing_identity() {
    let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    prepare(&mut p, 0x0040_0000, 7);
    assert!(matches!(
        stopped(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0x0040_0000, 8);
    assert!(matches!(
        stopped(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
}

#[test]
fn full_input_frame_and_zero_budget_precede_resource_lookup() {
    let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
    prepare(&mut p, 0x0040_0000, 7);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    assert!(matches!(
        stopped(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
}

#[test]
fn relocated_module_and_process_caches_keep_resource_identity_separate() {
    use ring3_core::execution::{GuestModule, ProcessOptions};
    let exe = icon_executable::guest();
    let mut dll = exe.clone();
    for (offset, value) in [
        (0xb4, 0x1000_0000),
        (0xa8, 0),
        (0x100, 0),
        (0x104, 0),
        (0x120, 0x3000),
        (0x124, 12),
        (0x1400, 0),
        (0x1404, 12),
    ] {
        icon_executable::put(&mut dll, offset, value);
    }
    dll[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    let mut p = Process32::load_with_options(
        &exe,
        40,
        ProcessOptions {
            modules: &[GuestModule {
                name: "extra.dll",
                bytes: &dll,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    dll.fill(0);
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
    assert_eq!(call(&mut p, 0x3000_0000, 7), 0x7800_0008);
    assert_eq!(call(&mut p, 0x0040_0000, 7), 0x7800_0004);
    let mut other = Process32::load(&exe, 32).unwrap();
    assert_eq!(call(&mut other, 0x0040_0000, 7), 0x7800_0004);
}
