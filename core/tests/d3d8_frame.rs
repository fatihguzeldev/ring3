#[path = "support/imported_executable.rs"]
mod imported_executable;

#[path = "support/d3d8_executable.rs"]
mod d3d8_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Process32, ProcessResult, ProcessStop, Register32, StopReason};

const PARAMETERS: u32 = 0x0040_2100;
const OUTPUT: u32 = 0x0040_2180;
const IDENTIFIER: u32 = 0x0040_2200;
const CAPS: u32 = 0x0040_2800;
const MODE_COUNT: u32 = 0x0040_28d4;
const MODE: u32 = 0x0040_28e0;
const DEVICE_TYPE_STATUS: u32 = 0x0040_28f0;
const DEVICE_FORMAT_STATUS: u32 = 0x0040_28f4;
const MULTISAMPLE_STATUS: u32 = 0x0040_28f8;
const TEXTURE_OUTPUT: u32 = 0x0040_2a00;
const LOCKED_RECT: u32 = 0x0040_2a10;
const LEVEL_DESC: u32 = 0x0040_2a20;
const FULLSCREEN_OUTPUT: u32 = 0x0040_2a40;
const FULLSCREEN_PARAMS: u32 = 0x0040_2a60;
const VIEWPORT: u32 = 0x0040_2bc0;

#[test]
fn executes_an_uninterrupted_guest_graphics_program() {
    for (width, height, color) in [(4, 3, 0xff12_3456_u32), (9, 7, 0xffed_cba9)] {
        let mut process =
            Process32::load(&d3d8_executable::pe32(width, height, color), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 12);
        assert_eq!(read(&process, PARAMETERS + 12), 1);
        assert_eq!(read(&process, IDENTIFIER), 0x676e_6972);
        assert_eq!(read(&process, CAPS), 1);
        assert_eq!(read(&process, CAPS + 12), 0x0008_0000);
        assert_eq!(read(&process, MODE_COUNT), 1);
        assert_eq!(
            read_bytes(&process, MODE, 16),
            [640_u32, 480, 0, 22]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(read(&process, DEVICE_TYPE_STATUS), 0);
        assert_eq!(read(&process, DEVICE_FORMAT_STATUS), 0);
        assert_eq!(read(&process, MULTISAMPLE_STATUS), 0);
        let frame = process.take_frame().unwrap();
        assert_eq!((frame.width, frame.height), (width, height));
        let [_, r, g, b] = color.to_be_bytes();
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel == [r, g, b, 255])
        );
    }
}

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn read_bytes(process: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn write(process: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    process.memory.write(u64::from(address), &bytes).unwrap();
}

fn call(process: &mut Process32, target: u32, args: &[u32]) -> ProcessResult {
    let stack = 0x1001_0000 - u32::try_from(args.len()).unwrap() * 4;
    write(process, stack, args);
    process.cpu.eip = 0x0040_1000;
    process.cpu.set_register(Register32::Esp, stack);
    process.cpu.set_register(Register32::Eax, target);
    process.run(10)
}

fn invoke(process: &mut Process32, target: u32, args: &[u32]) -> u32 {
    let result = call(process, target, args);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    process.cpu.register(Register32::Eax)
}

fn method(process: &Process32, object: u32, index: u32) -> u32 {
    read(process, read(process, object) + index * 4)
}

fn root() -> (Process32, u32) {
    let bytes = imported_executable::pe32(&[0xff, 0xd0, 0xcc], "d3d8.dll", &["Direct3DCreate8"]);
    let mut process = Process32::load(&bytes, 32).unwrap();
    let factory = read(&process, 0x0040_2060);
    let root = invoke(&mut process, factory, &[220]);
    assert_ne!(root, 0);
    write(
        &mut process,
        PARAMETERS,
        &[4, 3, 22, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0],
    );
    (process, root)
}

fn direct_call(process: &mut Process32, target: u32, args: &[u32]) -> u32 {
    let stack = 0x1000_ef00;
    let mut frame = vec![0x0040_10f0];
    frame.extend_from_slice(args);
    write(process, stack, &frame);
    process.cpu.eip = target;
    process.cpu.set_register(Register32::Esp, stack);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(result.api_calls, 1);
    process.cpu.register(Register32::Eax)
}

fn root_with_owned_window() -> (Process32, u32, u32) {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    assert_eq!(process.window_snapshots()[0].hwnd, hwnd);
    assert_eq!(process.window_snapshots()[0].parent, 0);
    let root = direct_call(&mut process, 0x7000_000c, &[220]);
    assert_ne!(root, 0);
    (process, root, hwnd)
}

#[test]
fn owned_window_can_create_fullscreen_d16_device_with_two_back_buffers() {
    let (mut process, root, hwnd) = root_with_owned_window();
    write(
        &mut process,
        FULLSCREEN_PARAMS,
        &[640, 480, 22, 2, 0, 1, hwnd, 0, 1, 80, 0, 0, 0],
    );
    let create = method(&process, root, 15);
    assert_eq!(
        direct_call(
            &mut process,
            create,
            &[root, 0, 1, hwnd, 0x20, FULLSCREEN_PARAMS, FULLSCREEN_OUTPUT]
        ),
        0
    );
    assert_eq!(read(&process, FULLSCREEN_PARAMS + 12), 2);
    let device = read(&process, FULLSCREEN_OUTPUT);
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        direct_call(
            &mut process,
            clear,
            &[device, 0, 0, 3, 0xff12_3456, 1_f32.to_bits(), 0]
        ),
        0
    );
    assert_eq!(direct_call(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let first = process.take_frame().unwrap();
    assert!(
        first
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x12, 0x34, 0x56, 255])
    );
    assert_eq!(direct_call(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let second = process.take_frame().unwrap();
    assert!(
        second
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 0, 255])
    );
    for (color, expected) in [
        (0xff12_3456, [0x12, 0x34, 0x56, 255]),
        (0xffab_cdef, [0xab, 0xcd, 0xef, 255]),
    ] {
        assert_eq!(
            direct_call(
                &mut process,
                clear,
                &[device, 0, 0, 3, color, 1_f32.to_bits(), 0]
            ),
            0
        );
        assert_eq!(direct_call(&mut process, present, &[device, 0, 0, 0, 0]), 0);
        let frame = process.take_frame().unwrap();
        assert_eq!((frame.width, frame.height), (640, 480));
        assert!(frame.rgba.chunks_exact(4).all(|pixel| pixel == expected));
    }
}

#[test]
fn fullscreen_creation_rejects_foreign_windows_modes_and_excess_buffers() {
    let (mut process, root, hwnd) = root_with_owned_window();
    let create = method(&process, root, 15);
    let base = [640, 480, 22, 2, 0, 1, hwnd, 0, 1, 80, 0, 0, 0];
    for (focus, slot, value) in [
        (hwnd + 4, 0, 640),
        (hwnd, 6, hwnd + 4),
        (0, 6, hwnd),
        (hwnd, 6, 1),
        (hwnd, 0, 800),
        (hwnd, 3, 3),
    ] {
        let mut params = base;
        params[slot] = value;
        write(&mut process, FULLSCREEN_PARAMS, &params);
        write(&mut process, FULLSCREEN_OUTPUT, &[0x1234_5678]);
        assert_eq!(
            direct_call(
                &mut process,
                create,
                &[
                    root,
                    0,
                    1,
                    focus,
                    0x20,
                    FULLSCREEN_PARAMS,
                    FULLSCREEN_OUTPUT
                ]
            ),
            0x8876_086c
        );
        assert_eq!(read(&process, FULLSCREEN_OUTPUT), 0x1234_5678);
    }
}

#[test]
fn create_accepts_only_direct3d_8_0_and_8_1_sdk_identities() {
    let bytes = imported_executable::pe32(&[0xff, 0xd0, 0xcc], "d3d8.dll", &["Direct3DCreate8"]);
    for version in [120, 220] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        let factory = read(&process, 0x0040_2060);
        assert_ne!(invoke(&mut process, factory, &[version]), 0);
        assert_eq!(invoke(&mut process, factory, &[version]), 0);
    }
    for version in [0, 119, 121, 219, 221, u32::MAX] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        let factory = read(&process, 0x0040_2060);
        assert_eq!(invoke(&mut process, factory, &[version]), 0);
    }
}

#[test]
fn adapter_count_reports_only_the_live_owned_root() {
    let (mut process, root) = root();
    assert_eq!(method(&process, root, 3), 0x7000_0ffc);
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    let count = method(&process, root, 4);
    assert_eq!(invoke(&mut process, count, &[root]), 1);
    assert_eq!(invoke(&mut process, count, &[root]), 1);
    assert_eq!(invoke(&mut process, count, &[root + 4]), 0);
    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(invoke(&mut process, count, &[root]), 0);
}

#[test]
fn adapter_mode_count_reports_only_the_owned_display() {
    let (mut process, root) = root();
    let count = method(&process, root, 6);
    assert_ne!(count, 0x7000_0ffc);
    assert_eq!(invoke(&mut process, count, &[root, 0]), 1);
    assert_eq!(invoke(&mut process, count, &[root, 0]), 1);
    assert_eq!(invoke(&mut process, count, &[root, 1]), 0);
    assert_eq!(invoke(&mut process, count, &[root + 4, 0]), 0);
    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(invoke(&mut process, count, &[root, 0]), 0);
}

#[test]
fn adapter_mode_reports_the_owned_display_profile() {
    let (mut process, root) = root();
    let enumerate = method(&process, root, 7);
    assert_ne!(enumerate, 0x7000_0ffc);
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    process.memory.write(u64::from(MODE), &[0xa5; 16]).unwrap();
    assert_eq!(invoke(&mut process, enumerate, &[root, 0, 0, MODE]), 0);
    assert_eq!(
        read_bytes(&process, MODE, 16),
        [640_u32, 480, 0, 22]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn current_display_mode_matches_the_only_enumerated_mode() {
    let (mut process, root) = root();
    let current = method(&process, root, 8);
    assert_ne!(current, 0x7000_0ffc);
    let enumerate = method(&process, root, 7);
    assert_eq!(invoke(&mut process, enumerate, &[root, 0, 0, MODE]), 0);
    let expected = read_bytes(&process, MODE, 16);
    process.memory.write(u64::from(MODE), &[0xa5; 16]).unwrap();
    assert_eq!(invoke(&mut process, current, &[root, 0, MODE]), 0);
    assert_eq!(read_bytes(&process, MODE, 16), expected);
}

#[test]
fn current_display_mode_rejects_invalid_identity_without_writing() {
    let (mut process, root) = root();
    let current = method(&process, root, 8);
    for args in [[root + 4, 0], [root, 1]] {
        process.memory.write(u64::from(MODE), &[0xa5; 16]).unwrap();
        assert_eq!(
            invoke(&mut process, current, &[args[0], args[1], MODE]),
            0x8876_086c
        );
        assert_eq!(read_bytes(&process, MODE, 16), [0xa5; 16]);
    }
    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(invoke(&mut process, current, &[root, 0, MODE]), 0x8876_086c);
    assert_eq!(read_bytes(&process, MODE, 16), [0xa5; 16]);
}

#[test]
fn current_display_mode_faults_before_any_prefix_write() {
    let (mut process, root) = root();
    let current = method(&process, root, 8);
    let partial = 0x0040_2ff8;
    process
        .memory
        .write(u64::from(partial), &[0xa5; 8])
        .unwrap();
    let result = call(&mut process, current, &[root, 0, partial]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(read_bytes(&process, partial, 8), [0xa5; 8]);
}

#[test]
fn invalid_or_released_roots_do_not_write_adapter_modes() {
    let (mut process, root) = root();
    let enumerate = method(&process, root, 7);
    for args in [[root + 4, 0, 0], [root, 1, 0], [root, 0, 1]] {
        process.memory.write(u64::from(MODE), &[0xa5; 16]).unwrap();
        assert_eq!(
            invoke(&mut process, enumerate, &[args[0], args[1], args[2], MODE]),
            0x8876_086c
        );
        assert_eq!(read_bytes(&process, MODE, 16), [0xa5; 16]);
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, enumerate, &[root, 0, 0, MODE]),
        0x8876_086c
    );
    assert_eq!(read_bytes(&process, MODE, 16), [0xa5; 16]);
}

#[test]
fn adapter_mode_faults_before_writing_any_prefix() {
    let (mut process, root) = root();
    let enumerate = method(&process, root, 7);
    let partial = 0x0040_2ff8;
    process
        .memory
        .write(u64::from(partial), &[0xa5; 8])
        .unwrap();
    let result = call(&mut process, enumerate, &[root, 0, 0, partial]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(read_bytes(&process, partial, 8), [0xa5; 8]);
}

#[test]
fn device_type_compatibility_matches_the_owned_formats() {
    let (mut process, root) = root();
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    let check = method(&process, root, 9);
    assert_ne!(check, 0x7000_0ffc);
    for windowed in [0, 1] {
        assert_eq!(
            invoke(&mut process, check, &[root, 0, 1, 22, 22, windowed]),
            0
        );
    }
    for args in [
        [root, 0, 2, 22, 22, 0],
        [root, 0, 3, 22, 22, 0],
        [root, 0, 1, 21, 22, 0],
        [root, 0, 1, 22, 21, 0],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086a);
    }
}

#[test]
fn invalid_or_released_roots_cannot_report_device_type_compatibility() {
    let (mut process, root) = root();
    let check = method(&process, root, 9);
    for args in [
        [root + 4, 0, 1, 22, 22, 0],
        [root, 1, 1, 22, 22, 0],
        [root, 0, 0, 22, 22, 0],
        [root, 0, 4, 22, 22, 0],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086c);
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, check, &[root, 0, 1, 22, 22, 0]),
        0x8876_086c
    );
}

#[test]
fn device_format_compatibility_matches_the_owned_render_target_surface() {
    let (mut process, root) = root();
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    let check = method(&process, root, 10);
    assert_ne!(check, 0x7000_0ffc);
    assert_eq!(invoke(&mut process, check, &[root, 0, 1, 22, 1, 1, 22]), 0);
    for args in [
        [root, 0, 2, 22, 1, 1, 22],
        [root, 0, 3, 22, 1, 1, 22],
        [root, 0, 1, 21, 1, 1, 22],
        [root, 0, 1, 22, 0, 1, 22],
        [root, 0, 1, 22, 1, 2, 22],
        [root, 0, 1, 22, 1, 1, 21],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086a);
    }
}

#[test]
fn device_format_reports_only_the_owned_normal_32_bit_textures() {
    let (mut process, root) = root();
    let check = method(&process, root, 10);
    assert_eq!(invoke(&mut process, check, &[root, 0, 1, 22, 0, 3, 22]), 0);
    assert_eq!(invoke(&mut process, check, &[root, 0, 1, 22, 0, 3, 21]), 0);
    for args in [
        [root, 0, 1, 22, 0, 3, 20],
        [root, 0, 1, 22, 1, 3, 22],
        [root, 0, 1, 22, 0, 2, 22],
        [root, 0, 2, 22, 0, 3, 22],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086a);
    }
}

#[test]
fn invalid_or_released_roots_cannot_report_device_format_compatibility() {
    let (mut process, root) = root();
    let check = method(&process, root, 10);
    for args in [
        [root + 4, 0, 1, 22, 1, 1, 22],
        [root, 1, 1, 22, 1, 1, 22],
        [root, 0, 0, 22, 1, 1, 22],
        [root, 0, 4, 22, 1, 1, 22],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086c);
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, check, &[root, 0, 1, 22, 1, 1, 22]),
        0x8876_086c
    );
}

#[test]
fn d16_depth_capability_matches_only_the_owned_color_target() {
    let (mut process, root) = root();
    let format = method(&process, root, 10);
    let match_depth = method(&process, root, 12);
    assert_eq!(invoke(&mut process, format, &[root, 0, 1, 22, 2, 1, 80]), 0);
    assert_eq!(
        invoke(&mut process, match_depth, &[root, 0, 1, 22, 22, 80]),
        0
    );
    for args in [
        [root, 0, 1, 22, 2, 1, 75],
        [root, 0, 1, 22, 2, 3, 80],
        [root, 0, 1, 21, 2, 1, 80],
    ] {
        assert_eq!(invoke(&mut process, format, &args), 0x8876_086a);
    }
    for args in [
        [root, 0, 1, 22, 22, 75],
        [root, 0, 1, 22, 21, 80],
        [root, 0, 2, 22, 22, 80],
    ] {
        assert_eq!(invoke(&mut process, match_depth, &args), 0x8876_086a);
    }
    assert_eq!(
        invoke(&mut process, match_depth, &[root, 1, 1, 22, 22, 80]),
        0x8876_086c
    );
}

fn depth_device() -> (Process32, u32) {
    let (mut process, root) = root();
    write(
        &mut process,
        PARAMETERS,
        &[4, 3, 22, 1, 0, 1, 1, 1, 1, 80, 0, 0, 0],
    );
    let create = method(&process, root, 15);
    assert_eq!(
        invoke(
            &mut process,
            create,
            &[root, 0, 1, 1, 0x20, PARAMETERS, OUTPUT]
        ),
        0
    );
    let device = read(&process, OUTPUT);
    (process, device)
}

fn draw_depth_triangle(process: &mut Process32, device: u32, z: f32, color: u32) {
    let vertices = 0x0040_2b00;
    write(
        process,
        vertices,
        &[
            0_f32.to_bits(),
            0_f32.to_bits(),
            z.to_bits(),
            1_f32.to_bits(),
            color,
            4_f32.to_bits(),
            0_f32.to_bits(),
            z.to_bits(),
            1_f32.to_bits(),
            color,
            0_f32.to_bits(),
            3_f32.to_bits(),
            z.to_bits(),
            1_f32.to_bits(),
            color,
        ],
    );
    let shader = method(process, device, 76);
    let draw = method(process, device, 72);
    assert_eq!(invoke(process, shader, &[device, 0x44]), 0);
    assert_eq!(invoke(process, draw, &[device, 4, 1, vertices, 20]), 0);
}

#[test]
fn d16_depth_clear_and_occlusion_keep_color_and_depth_independent() {
    let (mut process, device) = depth_device();
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 3, 0, 1_f32.to_bits(), 0]
        ),
        0
    );
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[255, 0, 0, 255]);

    assert_eq!(invoke(&mut process, clear, &[device, 0, 0, 1, 0, 0, 0]), 0);
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[0, 0, 0, 255]);

    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 2, 0, 1_f32.to_bits(), 0]
        ),
        0
    );
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[0, 0, 255, 255]);
}

#[test]
fn z_enable_render_state_controls_depth_tests_and_writes() {
    let (mut process, device) = depth_device();
    let set_state = method(&process, device, 50);
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, set_state, &[device, 7, 1]), 0);
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 3, 0, 1_f32.to_bits(), 0]
        ),
        0
    );
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    assert_eq!(invoke(&mut process, set_state, &[device, 7, 0]), 0);
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, set_state, &[device, 7, 1]), 0);
    draw_depth_triangle(&mut process, device, 0.5, 0xff00_ff00);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[0, 0, 255, 255]);
}

#[test]
fn invalid_z_enable_requests_preserve_the_owned_device_state() {
    let (mut process, device) = depth_device();
    let set_state = method(&process, device, 50);
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, set_state, &[device, 7, 0]), 0);
    for args in [[device, 7, 2], [device, 14, 1], [device + 4, 7, 1]] {
        assert_eq!(invoke(&mut process, set_state, &args), 0x8876_086c);
    }
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 3, 0, 1_f32.to_bits(), 0]
        ),
        0
    );
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[0, 0, 255, 255]);

    let release = method(&process, device, 2);
    assert_eq!(invoke(&mut process, release, &[device]), 0);
    assert_eq!(
        invoke(&mut process, set_state, &[device, 7, 1]),
        0x8876_086c
    );
}

#[test]
fn invalid_depth_clear_does_not_change_the_owned_surfaces() {
    let (mut process, device) = depth_device();
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 3, 0, 1_f32.to_bits(), 0]
        ),
        0
    );
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    for flags in [2, 3] {
        assert_eq!(
            invoke(
                &mut process,
                clear,
                &[device, 0, 0, flags, 0, f32::NAN.to_bits(), 0]
            ),
            0x8876_086c
        );
    }
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 0, 0, 4, 0, 1_f32.to_bits(), 0]
        ),
        0x8876_086c
    );
    draw_depth_triangle(&mut process, device, 0.75, 0xff00_00ff);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(&process.take_frame().unwrap().rgba[..4], &[255, 0, 0, 255]);
}

#[test]
fn set_viewport_clips_transformed_triangles_to_the_owned_subrect() {
    let (mut process, _, device) = create();
    let set_viewport = method(&process, device, 40);
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    write(
        &mut process,
        VIEWPORT,
        &[0, 0, 4, 3, 0_f32.to_bits(), 1_f32.to_bits()],
    );
    assert_eq!(invoke(&mut process, set_viewport, &[device, VIEWPORT]), 0);
    write(
        &mut process,
        VIEWPORT,
        &[1, 1, 2, 2, 0_f32.to_bits(), 1_f32.to_bits()],
    );
    assert_eq!(invoke(&mut process, set_viewport, &[device, VIEWPORT]), 0);
    assert_eq!(invoke(&mut process, clear, &[device, 0, 0, 1, 0, 0, 0]), 0);
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let frame = process.take_frame().unwrap();
    assert_eq!(&frame.rgba[0..4], &[0, 0, 0, 255]);
    assert_eq!(&frame.rgba[(4 + 1) * 4..(4 + 2) * 4], &[255, 0, 0, 255]);
}

#[test]
fn invalid_viewport_or_guest_fault_preserves_the_previous_clip() {
    let (mut process, _, device) = create();
    let set_viewport = method(&process, device, 40);
    let present = method(&process, device, 15);
    write(
        &mut process,
        VIEWPORT,
        &[1, 1, 2, 2, 0_f32.to_bits(), 1_f32.to_bits()],
    );
    assert_eq!(invoke(&mut process, set_viewport, &[device, VIEWPORT]), 0);
    for fields in [
        [3, 1, 2, 2, 0_f32.to_bits(), 1_f32.to_bits()],
        [0, 0, 0, 3, 0_f32.to_bits(), 1_f32.to_bits()],
        [u32::MAX, 0, 2, 3, 0_f32.to_bits(), 1_f32.to_bits()],
        [0, 0, 4, 3, 0.5_f32.to_bits(), 1_f32.to_bits()],
    ] {
        write(&mut process, VIEWPORT, &fields);
        assert_eq!(
            invoke(&mut process, set_viewport, &[device, VIEWPORT]),
            0x8876_086c
        );
    }
    let result = call(&mut process, set_viewport, &[device, 0x6000_0000]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    draw_depth_triangle(&mut process, device, 0.25, 0xffff_0000);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let frame = process.take_frame().unwrap();
    assert_eq!(&frame.rgba[0..4], &[0, 0, 0, 255]);
    assert_eq!(&frame.rgba[(4 + 1) * 4..(4 + 2) * 4], &[255, 0, 0, 255]);
}

#[test]
fn multisample_compatibility_matches_the_owned_surface() {
    let (mut process, root) = root();
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    let check = method(&process, root, 11);
    assert_ne!(check, 0x7000_0ffc);
    for windowed in [0, 1] {
        assert_eq!(
            invoke(&mut process, check, &[root, 0, 1, 22, windowed, 0]),
            0
        );
    }
    for args in [
        [root, 0, 1, 22, 0, 1],
        [root, 0, 1, 22, 0, 2],
        [root, 0, 1, 22, 0, 16],
        [root, 0, 2, 22, 0, 0],
        [root, 0, 3, 22, 0, 0],
        [root, 0, 1, 21, 0, 0],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086a);
    }
}

#[test]
fn invalid_or_released_roots_cannot_report_multisample_compatibility() {
    let (mut process, root) = root();
    let check = method(&process, root, 11);
    for args in [
        [root + 4, 0, 1, 22, 0, 0],
        [root, 1, 1, 22, 0, 0],
        [root, 0, 0, 22, 0, 0],
        [root, 0, 4, 22, 0, 0],
        [root, 0, 1, 22, 0, 17],
    ] {
        assert_eq!(invoke(&mut process, check, &args), 0x8876_086c);
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, check, &[root, 0, 1, 22, 0, 0]),
        0x8876_086c
    );
}

#[test]
fn adapter_identifier_reports_the_owned_virtual_adapter() {
    let (mut process, root) = root();
    let identifier = method(&process, root, 5);
    assert_ne!(identifier, 0x7000_0ffc);
    for flags in [0, 2] {
        process
            .memory
            .write(u64::from(IDENTIFIER), &[0xa5; 1068])
            .unwrap();
        assert_eq!(
            invoke(&mut process, identifier, &[root, 0, flags, IDENTIFIER]),
            0
        );

        let bytes = read_bytes(&process, IDENTIFIER, 1068);
        assert_eq!(&bytes[..6], b"ring3\0");
        assert!(bytes[6..512].iter().all(|byte| *byte == 0));
        assert_eq!(&bytes[512..542], b"Ring3 Virtual Display Adapter\0");
        assert!(bytes[542..].iter().all(|byte| *byte == 0));
    }
}

#[test]
fn invalid_or_released_roots_do_not_write_adapter_identifiers() {
    let (mut process, root) = root();
    let identifier = method(&process, root, 5);
    for args in [[root + 4, 0, 0], [root, 1, 0], [root, 0, 1]] {
        process
            .memory
            .write(u64::from(IDENTIFIER), &[0xa5; 1068])
            .unwrap();
        assert_eq!(
            invoke(
                &mut process,
                identifier,
                &[args[0], args[1], args[2], IDENTIFIER]
            ),
            0x8876_086c
        );
        assert!(
            read_bytes(&process, IDENTIFIER, 1068)
                .iter()
                .all(|byte| *byte == 0xa5)
        );
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, identifier, &[root, 0, 0, IDENTIFIER]),
        0x8876_086c
    );
    assert!(
        read_bytes(&process, IDENTIFIER, 1068)
            .iter()
            .all(|byte| *byte == 0xa5)
    );
}

#[test]
fn adapter_identifier_faults_before_writing_any_prefix() {
    let (mut process, root) = root();
    let identifier = method(&process, root, 5);
    let partial = 0x0040_2e00;
    process
        .memory
        .write(u64::from(partial), &[0xa5; 512])
        .unwrap();
    let result = call(&mut process, identifier, &[root, 0, 0, partial]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert!(
        read_bytes(&process, partial, 512)
            .iter()
            .all(|byte| *byte == 0xa5)
    );
}

#[test]
fn device_caps_report_only_the_owned_windowed_device() {
    let (mut process, root) = root();
    assert_ne!(method(&process, root, 12), 0x7000_0ffc);
    assert_eq!(method(&process, root, 14), 0x7000_0ffc);
    let get_caps = method(&process, root, 13);
    assert_ne!(get_caps, 0x7000_0ffc);
    for device_type in [2, 3] {
        process.memory.write(u64::from(CAPS), &[0xa5; 212]).unwrap();
        assert_eq!(
            invoke(&mut process, get_caps, &[root, 0, device_type, CAPS]),
            0x8876_086a
        );
        assert!(
            read_bytes(&process, CAPS, 212)
                .iter()
                .all(|byte| *byte == 0xa5)
        );
        assert_eq!(
            invoke(&mut process, get_caps, &[root, 0, device_type, u32::MAX]),
            0x8876_086a
        );
    }

    assert_eq!(invoke(&mut process, get_caps, &[root, 0, 1, CAPS]), 0);
    let caps = read_bytes(&process, CAPS, 212);
    assert_eq!(u32::from_le_bytes(caps[0..4].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(caps[4..8].try_into().unwrap()), 0);
    assert_eq!(
        u32::from_le_bytes(caps[12..16].try_into().unwrap()),
        0x0008_0000
    );
    assert!(caps[8..12].iter().all(|byte| *byte == 0));
    assert!(caps[16..28].iter().all(|byte| *byte == 0));
    assert_eq!(u32::from_le_bytes(caps[28..32].try_into().unwrap()), 0x400);
    assert!(caps[32..180].iter().all(|byte| *byte == 0));
    assert_eq!(u32::from_le_bytes(caps[180..184].try_into().unwrap()), 4096);
    assert!(caps[184..188].iter().all(|byte| *byte == 0));
    assert_eq!(u32::from_le_bytes(caps[188..192].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(caps[192..196].try_into().unwrap()), 256);
    assert!(caps[196..].iter().all(|byte| *byte == 0));
}

#[test]
fn invalid_or_released_roots_do_not_write_device_caps() {
    let (mut process, root) = root();
    let get_caps = method(&process, root, 13);
    for args in [[root + 4, 0, 1], [root, 1, 1], [root, 0, 0], [root, 0, 4]] {
        process.memory.write(u64::from(CAPS), &[0xa5; 212]).unwrap();
        assert_eq!(
            invoke(&mut process, get_caps, &[args[0], args[1], args[2], CAPS]),
            0x8876_086c
        );
        assert!(
            read_bytes(&process, CAPS, 212)
                .iter()
                .all(|byte| *byte == 0xa5)
        );
    }

    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(
        invoke(&mut process, get_caps, &[root, 0, 1, CAPS]),
        0x8876_086c
    );
    assert!(
        read_bytes(&process, CAPS, 212)
            .iter()
            .all(|byte| *byte == 0xa5)
    );
}

#[test]
fn device_caps_fault_before_writing_any_prefix() {
    let (mut process, root) = root();
    let get_caps = method(&process, root, 13);
    let partial = 0x0040_2f80;
    process
        .memory
        .write(u64::from(partial), &[0xa5; 128])
        .unwrap();
    let result = call(&mut process, get_caps, &[root, 0, 1, partial]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert!(
        read_bytes(&process, partial, 128)
            .iter()
            .all(|byte| *byte == 0xa5)
    );
}

fn create() -> (Process32, u32, u32) {
    let (mut process, root) = root();
    let create_device = method(&process, root, 15);
    assert_eq!(
        invoke(
            &mut process,
            create_device,
            &[root, 0, 1, 1, 0x20, PARAMETERS, OUTPUT]
        ),
        0
    );
    let device = read(&process, OUTPUT);
    (process, root, device)
}

#[test]
fn transformed_triangle_up_changes_the_presented_back_buffer() {
    let (mut process, root, device) = create();
    let get_caps = method(&process, root, 13);
    assert_eq!(invoke(&mut process, get_caps, &[root, 0, 1, CAPS]), 0);
    assert_eq!(read(&process, CAPS + 28) & 0x400, 0x400);
    let set_shader = method(&process, device, 76);
    let draw = method(&process, device, 72);
    assert_ne!(set_shader, 0x7000_0ffc);
    assert_ne!(draw, 0x7000_0ffc);
    assert_eq!(invoke(&mut process, set_shader, &[device, 0x44]), 0);

    let vertices = 0x0040_2b00;
    write(
        &mut process,
        vertices,
        &[
            0_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            4_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            0_f32.to_bits(),
            3_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
        ],
    );
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        invoke(&mut process, clear, &[device, 0, 0, 1, 0xff00_0000, 0, 0]),
        0
    );
    assert_eq!(invoke(&mut process, draw, &[device, 4, 1, vertices, 20]), 0);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let frame = process.take_frame().unwrap();
    assert_eq!(&frame.rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(
        &frame.rgba[(2 * 4 + 3) * 4..(2 * 4 + 4) * 4],
        &[0, 0, 0, 255]
    );
}

#[test]
fn transformed_triangle_up_culls_counterclockwise_winding() {
    let (mut process, _, device) = create();
    let vertices = 0x0040_2b00;
    write(
        &mut process,
        vertices,
        &[
            0_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            0_f32.to_bits(),
            3_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            4_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
        ],
    );
    let set_shader = method(&process, device, 76);
    let draw = method(&process, device, 72);
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, set_shader, &[device, 0x44]), 0);
    assert_eq!(invoke(&mut process, draw, &[device, 4, 1, vertices, 20]), 0);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert!(
        process
            .take_frame()
            .unwrap()
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 0, 255])
    );
}

#[test]
fn transformed_draw_rejects_bad_inputs_without_touching_the_back_buffer() {
    let (mut process, _, device) = create();
    let set_shader = method(&process, device, 76);
    let draw = method(&process, device, 72);
    assert_eq!(
        invoke(&mut process, set_shader, &[device, 0x04]),
        0x8876_086c
    );
    assert_eq!(invoke(&mut process, set_shader, &[device, 0x44]), 0);
    assert_eq!(
        invoke(&mut process, draw, &[device, 1, 1, 0x0040_2b00, 20]),
        0x8876_086c
    );
    assert_eq!(
        invoke(&mut process, draw, &[device, 4, 4097, 0x0040_2b00, 20]),
        0x8876_086c
    );
    assert_eq!(
        invoke(&mut process, draw, &[device, 4, 1, 0x0040_2b00, 19]),
        0x8876_086c
    );
    assert_eq!(invoke(&mut process, draw, &[device, 4, 0, 0, 20]), 0);
    let partial = 0x0040_2fd0;
    write(
        &mut process,
        partial,
        &[
            0_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            4_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
        ],
    );
    let result = call(&mut process, draw, &[device, 4, 1, partial, 20]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert!(
        process
            .take_frame()
            .unwrap()
            .rgba
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
}

#[test]
fn transformed_strip_interpolates_colors_and_draws_the_second_triangle() {
    let (mut process, _, device) = create();
    let vertices = 0x0040_2b00;
    write(
        &mut process,
        vertices,
        &[
            0_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_0000,
            4_f32.to_bits(),
            0_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xff00_ff00,
            0_f32.to_bits(),
            3_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xff00_00ff,
            4_f32.to_bits(),
            3_f32.to_bits(),
            0,
            1_f32.to_bits(),
            0xffff_ffff,
        ],
    );
    let set_shader = method(&process, device, 76);
    let draw = method(&process, device, 72);
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, set_shader, &[device, 0x44]), 0);
    assert_eq!(invoke(&mut process, draw, &[device, 5, 2, vertices, 20]), 0);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    let frame = process.take_frame().unwrap();
    let inside = &frame.rgba[20..24];
    assert!(
        inside[..3]
            .iter()
            .all(|channel| (1..=254).contains(channel))
    );
    assert_ne!(
        &frame.rgba[(2 * 4 + 3) * 4..(2 * 4 + 4) * 4],
        &[0, 0, 0, 255]
    );
}

#[test]
fn texture_owns_mip_pixels_and_releases_its_guest_memory() {
    let (mut process, root, device) = create();
    let create_texture = method(&process, device, 20);
    assert_ne!(create_texture, 0x7000_0ffc);
    assert_eq!(
        invoke(
            &mut process,
            create_texture,
            &[device, 4, 2, 0, 0, 22, 1, TEXTURE_OUTPUT]
        ),
        0
    );
    let texture = read(&process, TEXTURE_OUTPUT);
    assert_ne!(texture, 0);
    let level_count = method(&process, texture, 13);
    assert_eq!(invoke(&mut process, level_count, &[texture]), 3);

    let description = method(&process, texture, 14);
    for (level, width, height) in [(0, 4, 2), (1, 2, 1), (2, 1, 1)] {
        assert_eq!(
            invoke(&mut process, description, &[texture, level, LEVEL_DESC]),
            0
        );
        assert_eq!(
            read_bytes(&process, LEVEL_DESC, 32),
            [22, 3, 0, 1, width * height * 4, 0, width, height]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }

    let lock = method(&process, texture, 16);
    let unlock = method(&process, texture, 17);
    assert_eq!(
        invoke(&mut process, lock, &[texture, 0, LOCKED_RECT, 0, 0]),
        0
    );
    let pitch = read(&process, LOCKED_RECT);
    let pixels = read(&process, LOCKED_RECT + 4);
    assert_eq!(pitch, 16);
    process
        .memory
        .write(u64::from(pixels), &[1, 2, 3, 4])
        .unwrap();
    assert_eq!(
        invoke(&mut process, lock, &[texture, 0, LOCKED_RECT, 0, 0]),
        0x8876_086c
    );
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0x8876_086c);
    assert_eq!(
        invoke(&mut process, lock, &[texture, 0, LOCKED_RECT, 0, 0]),
        0
    );
    assert_eq!(
        read_bytes(&process, read(&process, LOCKED_RECT + 4), 4),
        [1, 2, 3, 4]
    );
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);

    let release = method(&process, texture, 2);
    let add_ref = method(&process, texture, 1);
    assert_eq!(invoke(&mut process, add_ref, &[texture]), 2);
    assert_eq!(invoke(&mut process, release, &[texture]), 1);
    assert_eq!(invoke(&mut process, release, &[texture]), 0);
    assert!(process.memory.read(u64::from(pixels), &mut [0]).is_err());
    assert_eq!(invoke(&mut process, level_count, &[texture]), 0x8876_086c);
    let device_release = method(&process, device, 2);
    let root_release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, device_release, &[device]), 0);
    assert_eq!(invoke(&mut process, root_release, &[root]), 0);
}

#[test]
fn alpha_texture_preserves_32_bit_bgra_pixels_and_reports_its_format() {
    let (mut process, _, device) = create();
    let create_texture = method(&process, device, 20);
    assert_eq!(
        invoke(
            &mut process,
            create_texture,
            &[device, 2, 1, 1, 0, 21, 1, TEXTURE_OUTPUT]
        ),
        0
    );
    let texture = read(&process, TEXTURE_OUTPUT);
    let level_desc = method(&process, texture, 14);
    assert_eq!(
        invoke(&mut process, level_desc, &[texture, 0, LEVEL_DESC]),
        0
    );
    assert_eq!(read(&process, LEVEL_DESC), 21);
    assert_eq!(read(&process, LEVEL_DESC + 16), 8);

    let lock = method(&process, texture, 16);
    let unlock = method(&process, texture, 17);
    assert_eq!(
        invoke(&mut process, lock, &[texture, 0, LOCKED_RECT, 0, 0]),
        0
    );
    let pixels = read(&process, LOCKED_RECT + 4);
    assert_eq!(read(&process, LOCKED_RECT), 8);
    process
        .memory
        .write(u64::from(pixels), &[0x33, 0x22, 0x11, 0x7f])
        .unwrap();
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);
    assert_eq!(
        invoke(&mut process, lock, &[texture, 0, LOCKED_RECT, 0, 0]),
        0
    );
    assert_eq!(read_bytes(&process, pixels, 4), [0x33, 0x22, 0x11, 0x7f]);
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);
    let release = method(&process, texture, 2);
    assert_eq!(invoke(&mut process, release, &[texture]), 0);
}

#[test]
fn unsupported_texture_requests_preserve_output_and_device_lifetime() {
    let (mut process, _, device) = create();
    let create_texture = method(&process, device, 20);
    for args in [
        [0, 2, 1, 0, 22, 1],
        [4, 2, 1, 1, 22, 1],
        [4, 2, 1, 0, 20, 1],
        [4, 2, 1, 0, 22, 3],
        [u32::MAX, u32::MAX, 1, 0, 22, 1],
    ] {
        write(&mut process, TEXTURE_OUTPUT, &[0x1234_5678]);
        assert_eq!(
            invoke(
                &mut process,
                create_texture,
                &[
                    device,
                    args[0],
                    args[1],
                    args[2],
                    args[3],
                    args[4],
                    args[5],
                    TEXTURE_OUTPUT
                ]
            ),
            0x8876_086c
        );
        assert_eq!(read(&process, TEXTURE_OUTPUT), 0x1234_5678);
    }
    let release = method(&process, device, 2);
    assert_eq!(invoke(&mut process, release, &[device]), 0);
}

#[test]
fn texture_failures_preserve_output_lock_state_and_page_capacity() {
    let (mut process, _, device) = create();
    let create_texture = method(&process, device, 20);
    let pages = process.memory.mapped_pages();
    write(&mut process, TEXTURE_OUTPUT, &[0x1234_5678]);
    assert_eq!(
        invoke(
            &mut process,
            create_texture,
            &[device, 512, 512, 1, 0, 22, 1, TEXTURE_OUTPUT]
        ),
        0x8876_017c
    );
    assert_eq!(read(&process, TEXTURE_OUTPUT), 0x1234_5678);
    assert_eq!(process.memory.mapped_pages(), pages);

    let bad_output = call(
        &mut process,
        create_texture,
        &[device, 4, 2, 1, 0, 22, 1, 0x0040_1000],
    );
    assert!(matches!(
        bad_output.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.memory.mapped_pages(), pages);
    assert_eq!(
        invoke(
            &mut process,
            create_texture,
            &[device, 4, 2, 1, 0, 22, 1, TEXTURE_OUTPUT]
        ),
        0
    );
    let texture = read(&process, TEXTURE_OUTPUT);
    let lock = method(&process, texture, 16);
    let unlock = method(&process, texture, 17);
    write(&mut process, PARAMETERS, &[1, 0, 3, 2]);
    assert_eq!(
        invoke(
            &mut process,
            lock,
            &[texture, 0, LOCKED_RECT, PARAMETERS, 0]
        ),
        0
    );
    assert_eq!(read(&process, LOCKED_RECT), 16);
    assert_eq!(read(&process, LOCKED_RECT + 4), texture + 4096 + 4);
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);

    write(&mut process, PARAMETERS, &[1, 0, 5, 2]);
    write(&mut process, LOCKED_RECT, &[0x1234_5678, 0x8765_4321]);
    assert_eq!(
        invoke(
            &mut process,
            lock,
            &[texture, 0, LOCKED_RECT, PARAMETERS, 0]
        ),
        0x8876_086c
    );
    assert_eq!(read(&process, LOCKED_RECT), 0x1234_5678);
    assert_eq!(read(&process, LOCKED_RECT + 4), 0x8765_4321);
    assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0x8876_086c);
}

#[test]
fn clear_present_uses_com_slots_and_preserves_presented_snapshot() {
    let (mut process, _, device) = create();
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        invoke(&mut process, clear, &[device, 0, 0, 1, 0xff11_2233, 0, 0]),
        0
    );
    write(&mut process, PARAMETERS, &[u32::MAX, 1, 2, 5]);
    assert_eq!(
        invoke(
            &mut process,
            clear,
            &[device, 1, PARAMETERS, 1, 0xffaa_bbcc, 0, 0]
        ),
        0
    );
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert_eq!(invoke(&mut process, clear, &[device, 0, 0, 1, 0, 0, 0]), 0);
    let frame = process.take_frame().unwrap();
    assert_eq!((frame.width, frame.height), (4, 3));
    for y in 0..3 {
        for x in 0..4 {
            let color = if y >= 1 && x < 2 {
                [0xaa, 0xbb, 0xcc, 255]
            } else {
                [0x11, 0x22, 0x33, 255]
            };
            assert_eq!(frame.rgba[(y * 4 + x) * 4..(y * 4 + x + 1) * 4], color);
        }
    }
    assert!(process.take_frame().is_none());
}

#[test]
fn device_retains_root_until_final_release() {
    let (mut process, root, device) = create();
    let root_release = method(&process, root, 2);
    let device_add = method(&process, device, 1);
    let device_release = method(&process, device, 2);
    assert_eq!(invoke(&mut process, root_release, &[root]), 1);
    assert_eq!(invoke(&mut process, device_add, &[device]), 2);
    assert_eq!(invoke(&mut process, device_release, &[device]), 1);
    assert_eq!(invoke(&mut process, device_release, &[device]), 0);
    let factory = read(&process, 0x0040_2060);
    assert_ne!(invoke(&mut process, factory, &[220]), 0);
}

#[test]
fn an_uncleared_x8_back_buffer_presents_as_opaque() {
    let (mut process, _, device) = create();
    let present = method(&process, device, 15);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert!(
        process
            .take_frame()
            .unwrap()
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 0, 255])
    );
}

#[test]
fn invalid_output_pointer_does_not_create_a_device_or_change_references() {
    let (mut process, root) = root();
    let create = method(&process, root, 15);
    let result = call(
        &mut process,
        create,
        &[root, 0, 1, 1, 0x20, PARAMETERS, 0x0040_1000],
    );
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(
        invoke(
            &mut process,
            create,
            &[root, 0, 1, 1, 0x20, PARAMETERS, OUTPUT]
        ),
        0
    );
    assert_ne!(read(&process, OUTPUT), 0);
    assert!(process.memory.write(u64::from(root), &[0]).is_err());
    let table = read(&process, root);
    assert!(process.memory.write(u64::from(table), &[0]).is_err());
}

#[test]
fn oversized_or_unsupported_device_parameters_leave_output_unchanged() {
    for (slot, value) in [(0, u32::MAX), (2, 21), (4, 1), (7, 0), (8, 1)] {
        let (mut process, root) = root();
        let create = method(&process, root, 15);
        write(&mut process, PARAMETERS + slot * 4, &[value]);
        write(&mut process, OUTPUT, &[0x1234]);
        assert_eq!(
            invoke(
                &mut process,
                create,
                &[root, 0, 1, 1, 0x20, PARAMETERS, OUTPUT]
            ),
            0x8876_086c
        );
        assert_eq!(read(&process, OUTPUT), 0x1234);
    }
}

#[test]
fn faulty_clear_and_unknown_com_method_cannot_produce_a_successful_frame() {
    let (mut process, _, device) = create();
    let clear = method(&process, device, 36);
    let present = method(&process, device, 15);
    assert_eq!(
        invoke(&mut process, clear, &[device, 0, 0, 1, 0xff12_3456, 0, 0]),
        0
    );
    let result = call(&mut process, clear, &[device, 1, u32::MAX - 7, 1, 0, 0, 0]);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(invoke(&mut process, present, &[device, 0, 0, 0, 0]), 0);
    assert!(
        process
            .take_frame()
            .unwrap()
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x12, 0x34, 0x56, 255])
    );
    let unsupported = method(&process, device, 0);
    let result = call(&mut process, unsupported, &[device, 0, 0]);
    assert_eq!(
        result.reason,
        ProcessStop::UnsupportedApi {
            address: unsupported
        }
    );
    assert_eq!(result.api_calls, 0);
}
