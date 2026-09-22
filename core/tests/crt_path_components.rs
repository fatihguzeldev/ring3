#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/path_component_cases.rs"]
mod path_component_cases;

use path_component_cases::{OUTPUTS, SOURCE};
use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0190;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    let mut p = Process32::load(&path_component_cases::executable(), 128).unwrap();
    for address in OUTPUTS {
        p.memory.write(u64::from(address), &[0x55; 256]).unwrap();
    }
    p
}

fn prepare(p: &mut Process32, source: u32, outputs: [u32; 4]) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    let frame: Vec<_> = [0x0040_1000, source]
        .into_iter()
        .chain(outputs)
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
    p.cpu
}

fn read(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn success(p: &mut Process32, target: u32) {
    let mut expected = p.cpu;
    let pages = p.memory.mapped_pages();
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 4);
    assert_eq!(p.cpu, expected);
    assert_eq!(p.memory.mapped_pages(), pages);
}

fn failure(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let before = OUTPUTS.map(|address| read(p, address, 256));
    let pages = p.memory.mapped_pages();
    let result = p.run(1);
    if unsupported {
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    } else {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(OUTPUTS.map(|address| read(p, address, 256)), before);
}

#[test]
fn imported_path_splitting_preserves_bytes_and_cdecl_void_state() {
    path_component_cases::verify();
}

#[test]
fn each_output_is_optional_and_all_existing_code_pages_keep_single_bytes() {
    for selector in [-3_i32, -2, 0, 1252, 437] {
        for mask in 0..16 {
            let mut p = process();
            prepare(&mut p, selector.cast_unsigned(), [0; 4]);
            p.cpu.eip = 0x7000_013c;
            assert_eq!(p.run(1).api_calls, 1);
            p.memory
                .write(u64::from(SOURCE), b"D:\\d\x81\\f\xff.x\0")
                .unwrap();
            let outputs =
                std::array::from_fn(|i| if mask & (1 << i) == 0 { 0 } else { OUTPUTS[i] });
            prepare(&mut p, SOURCE, outputs);
            success(&mut p, 0x0040_1000);
            for (i, expected) in [b"D:\0".as_slice(), b"\\d\x81\\\0", b"f\xff\0", b".x\0"]
                .into_iter()
                .enumerate()
            {
                let actual = read(&p, OUTPUTS[i], expected.len() + 1);
                if outputs[i] == 0 {
                    assert_eq!(actual, vec![0x55; expected.len() + 1]);
                } else {
                    assert_eq!(&actual[..expected.len()], expected);
                    assert_eq!(actual[expected.len()], 0x55);
                }
            }
        }
    }
}

#[test]
fn requested_component_limits_include_nul_and_skipped_components_need_no_buffer() {
    let mut p = process();
    let source = 0x3000_0000;
    p.memory
        .map_zeroed(u64::from(source), 4096, Permissions::READ_WRITE)
        .unwrap();
    for index in 1..4 {
        for length in [254, 255, 256] {
            let mut bytes = vec![b'a'; length];
            if index == 1 {
                bytes[length - 1] = b'\\';
            }
            if index == 3 {
                bytes[0] = b'.';
            }
            bytes.push(0);
            p.memory.write(u64::from(source), &bytes).unwrap();
            prepare(&mut p, source, OUTPUTS);
            if length == 256 {
                failure(&mut p, true);
                let mut outputs = OUTPUTS;
                outputs[index] = 0;
                prepare(&mut p, source, outputs);
                success(&mut p, 0x0040_1000);
            } else {
                success(&mut p, 0x0040_1000);
                assert_eq!(read(&p, OUTPUTS[index], length + 1), bytes);
            }
        }
    }
}

#[test]
fn source_scan_and_output_spans_check_page_and_address_boundaries() {
    let mut p = process();
    let source = 0x3000_0000;
    p.memory
        .map_zeroed(u64::from(source), 65536, Permissions::READ_WRITE)
        .unwrap();
    let mut bytes = vec![b'a'; 65536];
    bytes[65535] = 0;
    p.memory.write(u64::from(source), &bytes).unwrap();
    prepare(&mut p, source, [0; 4]);
    success(&mut p, 0x0040_1000);
    p.memory.write(u64::from(source) + 65535, b"a").unwrap();
    prepare(&mut p, source, [0; 4]);
    failure(&mut p, true);
    prepare(&mut p, source + 65535, OUTPUTS);
    failure(&mut p, false);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    prepare(&mut p, u32::MAX, OUTPUTS);
    failure(&mut p, false);
    p.memory.write(u64::from(SOURCE), b"x\0").unwrap();
    prepare(&mut p, SOURCE, [0, 0, u32::MAX, 0]);
    failure(&mut p, false);
    prepare(&mut p, SOURCE, [u32::MAX, 0, 0, 0]);
    success(&mut p, 0x0040_1000);
    assert_eq!(read(&p, u32::MAX, 1), [0]);
}

#[test]
fn complete_frames_and_late_output_faults_preserve_prior_outputs_and_allow_retry() {
    let mut p = process();
    p.memory
        .write(u64::from(SOURCE), b"A:\\dir\\file.ext\0")
        .unwrap();
    prepare(&mut p, SOURCE, OUTPUTS);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_ffec);
    failure(&mut p, false);
    let late = 0x3000_0000;
    p.memory
        .map_zeroed(u64::from(late), 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, SOURCE, [OUTPUTS[0], OUTPUTS[1], OUTPUTS[2], late]);
    failure(&mut p, false);
    assert_eq!(read(&p, late, 5), [0; 5]);
    p.memory
        .protect(u64::from(late), 4096, Permissions::READ_WRITE)
        .unwrap();
    success(&mut p, 0x0040_1000);
    assert_eq!(read(&p, late, 5), b".ext\0");
    prepare(&mut p, 0, OUTPUTS);
    failure(&mut p, true);
}

#[test]
fn overlap_includes_terminators_but_adjacent_outputs_and_saved_return_aliases_work() {
    let mut p = process();
    p.memory.write(u64::from(SOURCE), b"A:f.x\0").unwrap();
    for outputs in [
        [SOURCE, 0, 0, 0],
        [0, SOURCE + 5, 0, 0],
        [OUTPUTS[0], OUTPUTS[0] + 2, 0, 0],
    ] {
        prepare(&mut p, SOURCE, outputs);
        failure(&mut p, true);
        assert_eq!(read(&p, SOURCE, 6), b"A:f.x\0");
    }
    prepare(&mut p, SOURCE, [OUTPUTS[0], OUTPUTS[0] + 3, 0, 0]);
    success(&mut p, 0x0040_1000);
    assert_eq!(read(&p, OUTPUTS[0], 5), [b'A', b':', 0, 0, 0x55]);
    p.memory.write(u64::from(SOURCE), b"\xf0\x10@.x\0").unwrap();
    prepare(&mut p, SOURCE, [0, 0, STACK, 0]);
    success(&mut p, 0x0040_10f0);
    p.memory.write(u64::from(SOURCE), b"X\0").unwrap();
    for cell in [0x7000_2020, 0x7ffd_e034] {
        prepare(&mut p, SOURCE, [0, 0, cell, 0]);
        success(&mut p, 0x0040_1000);
        assert_eq!(read(&p, cell, 2), b"X\0");
    }
}
