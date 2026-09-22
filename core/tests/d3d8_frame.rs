#[path = "support/imported_executable.rs"]
mod imported_executable;

#[path = "support/d3d8_executable.rs"]
mod d3d8_executable;

use ring3_core::execution::{Process32, ProcessResult, ProcessStop, Register32, StopReason};

const PARAMETERS: u32 = 0x0040_2100;
const OUTPUT: u32 = 0x0040_2180;
const IDENTIFIER: u32 = 0x0040_2200;

#[test]
fn executes_an_uninterrupted_guest_graphics_program() {
    for (width, height, color) in [(4, 3, 0xff12_3456_u32), (9, 7, 0xffed_cba9)] {
        let mut process =
            Process32::load(&d3d8_executable::pe32(width, height, color), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 5);
        assert_eq!(read(&process, PARAMETERS + 12), 1);
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
    assert_eq!(method(&process, root, 6), 0x7000_0ffc);
    let count = method(&process, root, 4);
    assert_eq!(invoke(&mut process, count, &[root]), 1);
    assert_eq!(invoke(&mut process, count, &[root]), 1);
    assert_eq!(invoke(&mut process, count, &[root + 4]), 0);
    let release = method(&process, root, 2);
    assert_eq!(invoke(&mut process, release, &[root]), 0);
    assert_eq!(invoke(&mut process, count, &[root]), 0);
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
