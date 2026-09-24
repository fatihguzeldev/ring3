use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

pub const DATA: u32 = 0x0040_2180;
pub const HEADER: u32 = DATA + 129;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

fn code() -> Vec<u8> {
    let mut code = Vec::new();
    for value in [0, DATA, 0x700, 0x0040_0000] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&0x0040_2060_u32.to_le_bytes());
    store(&mut code, DATA + 16);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    for value in [0, DATA + 4, DATA + 64] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 12]);
    store(&mut code, DATA + 20);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 24);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&(DATA + 4).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    push(&mut code, super::dinput_mouse_format_cases::FORMAT);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 44]);
    store(&mut code, DATA + 28);
    for index in 0..3 {
        push(&mut code, HEADER + index * 32);
        push(&mut code, 3);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 20]);
        store(&mut code, DATA + 32 + index * 4);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 44);
    code.push(0xcc);
    code
}

pub fn imported_mouse_axis_granularity_across_budgets() {
    let bytes = super::imported_executable::pe32(&code(), "dinput.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        p.memory
            .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        super::dinput_mouse_format_cases::standard(&mut p.memory, false);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ)
            .unwrap();
        p.memory
            .write(u64::from(DATA + 64), &super::dinput_mouse_cases::MOUSE)
            .unwrap();
        for index in 0..3 {
            let address = HEADER + index * 32;
            p.memory.write(u64::from(address - 1), &[0xa5; 22]).unwrap();
            let header: Vec<_> = [20_u32, 16, index * 4, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            p.memory.write(u64::from(address), &header).unwrap();
        }
        let stack = p.cpu.register(Register32::Esp);
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            assert!(counts.0 < 1000);
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        let mut outputs = [0; 32];
        p.memory.read(u64::from(DATA + 16), &mut outputs).unwrap();
        assert_eq!(outputs, [0; 32]);
        let mut headers = Vec::new();
        for (index, value) in (0..).zip([1, 1, 120]) {
            let mut actual = [0; 22];
            p.memory
                .read(u64::from(HEADER + index * 32 - 1), &mut actual)
                .unwrap();
            let expected: Vec<_> = [20_u32, 16, index * 4, 1, value]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert_eq!(&actual[1..21], expected);
            assert_eq!((actual[0], actual[21]), (0xa5, 0xa5));
            headers.extend(actual);
        }
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 8);
        results.push((p.cpu, counts, outputs, headers));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
