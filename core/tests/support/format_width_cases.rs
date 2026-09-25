use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_2180;
const VALUES: u32 = 0x0040_21c0;
const TEXT: u32 = 0x0040_2300;
const OUTPUT: u32 = 0x6000_0000;
const VS: u32 = 0x7000_0178;
const SPRINTF: u32 = 0x7000_01ac;
const SNPRINTF: u32 = 0x7000_01bc;
const WINDOWS: u32 = 0x7000_025c;

fn process() -> Process32 {
    let mut p = Process32::load(&super::formatting_executable::pe32(), 256).unwrap();
    p.memory
        .map_zeroed(u64::from(OUTPUT), 69632, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(TEXT), b"long\0").unwrap();
    p
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn prepare(p: &mut Process32, api: u32, format: &[u8], values: &[u32], cap: u32) -> Cpu32 {
    p.memory.write(u64::from(FORMAT), format).unwrap();
    words(p, STACK, &[0x0040_1000, OUTPUT]);
    match api {
        VS => {
            words(p, STACK + 8, &[cap, FORMAT, VALUES]);
            words(p, VALUES, values);
        }
        SNPRINTF => {
            words(p, STACK + 8, &[cap, FORMAT]);
            words(p, STACK + 16, values);
        }
        SPRINTF | WINDOWS => {
            words(p, STACK + 8, &[FORMAT]);
            words(p, STACK + 12, values);
        }
        _ => unreachable!(),
    }
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu
}

fn read(p: &Process32, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

fn success(p: &mut Process32, mut expected: Cpu32, result: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, expected);
}

fn failure(p: &mut Process32, before: Cpu32, fault: bool) {
    let output = read(p, 65537);
    let run = p.run(1);
    if fault {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    } else {
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(read(p, 65537), output);
}

pub fn imported_width_runs_whole_or_stepwise() {
    for (format, value, expected) in [
        (b"%2d %s\0".as_slice(), 0_u32, b" 0 ok\0".as_slice()),
        (b"%02x %s\0", 3, b"03 ok\0"),
    ] {
        let mut executable = super::formatting_executable::pe32();
        executable[0x580..0x580 + format.len()].copy_from_slice(format);
        executable[0x5c0..0x5c4].copy_from_slice(&value.to_le_bytes());
        executable[0x5c4..0x5c8].copy_from_slice(&0x0040_21a0_u32.to_le_bytes());
        for budget in [1, 40] {
            let mut p = Process32::load(&executable, 64).unwrap();
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 40);
            }
            assert_eq!(counts, (7, 1));
            assert_eq!(p.cpu.register(Register32::Eax), 5);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            let mut output = [0; 6];
            p.memory.read(0x0040_2200, &mut output).unwrap();
            assert_eq!(&output, expected);
        }
    }
}

pub fn minimum_widths_preserve_values_and_wrapper_rules() {
    let mut p = process();
    for api in [VS, SPRINTF, SNPRINTF, WINDOWS] {
        let before = prepare(
            &mut p,
            api,
            b"%2d/%5i/%1s/%4s/%6s/%12d/%12u/%4x/%4X/%%\0",
            &[
                0,
                (-12_i32).cast_unsigned(),
                TEXT,
                TEXT,
                TEXT,
                0x8000_0000,
                u32::MAX,
                0xab,
                0xab,
            ],
            128,
        );
        let expected = b" 0/  -12/long/long/  long/ -2147483648/  4294967295/  ab/  AB/%\0";
        success(&mut p, before, u32::try_from(expected.len() - 1).unwrap());
        assert_eq!(read(&p, expected.len()), expected);
    }
    p.memory.write(u64::from(TEXT), b"\x80\xff\0").unwrap();
    for api in [VS, SPRINTF, SNPRINTF] {
        let before = prepare(
            &mut p,
            api,
            b"%3c/%4s/%2s\0",
            &[0x1234_0100, TEXT, TEXT + 2],
            128,
        );
        success(&mut p, before, 11);
        assert_eq!(read(&p, 12), b"  \0/  \x80\xff/  \0");
    }
    for api in [VS, SNPRINTF] {
        for (cap, expected, result) in [
            (1, b" !!!!!!!!!".as_slice(), u32::MAX),
            (4, b"   7!!!!!!", if api == VS { 4 } else { u32::MAX }),
            (5, b"   7\0!!!!!", 4),
        ] {
            p.memory.write(u64::from(OUTPUT), &[b'!'; 10]).unwrap();
            let before = prepare(&mut p, api, b"%4d\0", &[7], cap);
            success(&mut p, before, result);
            assert_eq!(read(&p, 10), expected);
        }
    }
}

pub fn zero_padding_preserves_signs_and_wrapper_rules() {
    let mut p = process();
    for api in [VS, SPRINTF, SNPRINTF, WINDOWS] {
        let before = prepare(
            &mut p,
            api,
            b"%05d/%012i/%012u/%02x/%04X/%02x/%0d/%0003u/%%\0",
            &[
                (-12_i32).cast_unsigned(),
                0x8000_0000,
                u32::MAX,
                3,
                0xab,
                0xabc,
                7,
                0,
            ],
            128,
        );
        let expected = b"-0012/-02147483648/004294967295/03/00AB/abc/7/000/%\0";
        success(&mut p, before, u32::try_from(expected.len() - 1).unwrap());
        assert_eq!(read(&p, expected.len()), expected);
    }
    for api in [VS, SNPRINTF] {
        for (cap, expected, result) in [
            (0, b"!!!!!!!!!!".as_slice(), u32::MAX),
            (1, b"-!!!!!!!!!", u32::MAX),
            (4, b"-007!!!!!!", if api == VS { 4 } else { u32::MAX }),
            (5, b"-007\0!!!!!", 4),
        ] {
            p.memory.write(u64::from(OUTPUT), &[b'!'; 10]).unwrap();
            let before = prepare(&mut p, api, b"%04d\0", &[(-7_i32).cast_unsigned()], cap);
            success(&mut p, before, result);
            assert_eq!(read(&p, 10), expected);
        }
    }
}

pub fn widths_obey_total_output_bounds() {
    let mut p = process();
    for (api, format, width, overflow, combined, fill) in [
        (
            VS,
            b"%65536d\0".as_slice(),
            65536,
            b"%65537d\0".as_slice(),
            b"x%65536d\0".as_slice(),
            b' ',
        ),
        (WINDOWS, b"%1023d\0", 1023, b"%1024d\0", b"x%1023d\0", b' '),
        (
            VS,
            b"%065536d\0",
            65536,
            b"%065537d\0",
            b"x%065536d\0",
            b'0',
        ),
        (
            WINDOWS,
            b"%01023d\0",
            1023,
            b"%01024d\0",
            b"x%01023d\0",
            b'0',
        ),
    ] {
        let before = prepare(&mut p, api, format, &[7], 65537);
        success(&mut p, before, width);
        let output = read(&p, width as usize + 1);
        assert!(
            output[..width as usize - 1]
                .iter()
                .all(|byte| *byte == fill)
        );
        assert_eq!(&output[width as usize - 1..], b"7\0");
        for format in [
            overflow,
            combined,
            b"%999999999999999999999999999999999999d\0",
        ] {
            let before = prepare(&mut p, api, format, &[7], 1);
            failure(&mut p, before, false);
        }
    }
}

pub fn unsupported_widths_and_late_faults_are_atomic() {
    let mut p = process();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 128]).unwrap();
    for format in [
        b"%02s\0".as_slice(),
        b"%0c\0",
        b"%0\0",
        b"%0%\0",
        b"%0002s\0",
        b"%2%\0",
        b"%2\0",
        b"%+2d\0",
        b"%-2d\0",
        b"% 2d\0",
        b"%#2x\0",
        b"%*d\0",
        b"%2.1d\0",
        b"%2ld\0",
        b"%04d%f\0",
        b"%0.2d\0",
    ] {
        let before = prepare(&mut p, VS, format, &[7, 0], 1);
        failure(&mut p, before, false);
    }
    let before = prepare(&mut p, WINDOWS, b"%2c\0", &[65], 128);
    failure(&mut p, before, false);
    let before = prepare(&mut p, VS, b"%04d%2s\0", &[7, 0x5000_0000], 1);
    failure(&mut p, before, true);
    prepare(&mut p, VS, b"%04d\0", &[7], 128);
    words(&mut p, STACK + 4, &[OUTPUT + 4094]);
    p.memory
        .protect(u64::from(OUTPUT) + 4096, 4096, Permissions::READ)
        .unwrap();
    let before = p.cpu;
    failure(&mut p, before, true);
}
