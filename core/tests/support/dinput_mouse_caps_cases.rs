use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

pub const DATA: u32 = 0x0040_2180;
pub const CAPS: u32 = DATA + 97;
pub const LEGACY: u32 = DATA + 161;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

pub fn imported_mouse_capabilities_across_budgets() {
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
    for (output, offset) in [(CAPS, 28), (LEGACY, 32)] {
        push(&mut code, output);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 12]);
        store(&mut code, DATA + offset);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 36);
    code.push(0xcc);
    let bytes = super::imported_executable::pe32(&code, "dinput.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        p.memory
            .write(u64::from(DATA + 64), &super::dinput_mouse_cases::MOUSE)
            .unwrap();
        for (address, size) in [(CAPS, 44_u32), (LEGACY, 24)] {
            p.memory.write(u64::from(address - 1), &[0xa5; 46]).unwrap();
            p.memory
                .write(u64::from(address), &size.to_le_bytes())
                .unwrap();
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
        let mut outputs = [0; 24];
        p.memory.read(u64::from(DATA + 16), &mut outputs).unwrap();
        assert_eq!(outputs, [0; 24]);
        let mut caps = Vec::new();
        for (address, size) in [(CAPS, 44_u32), (LEGACY, 24)] {
            let mut result = [0; 46];
            p.memory.read(u64::from(address - 1), &mut result).unwrap();
            let words = [size, 5, 0x202, 3, 8, 0, 0, 0, 0, 0, 0];
            let expected: Vec<_> = words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .take(size as usize)
                .collect();
            assert_eq!(&result[1..=size as usize], expected);
            assert_eq!(result[0], 0xa5);
            assert!(result[1 + size as usize..].iter().all(|byte| *byte == 0xa5));
            caps.extend(result);
        }
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 6);
        results.push((p.cpu, counts, outputs, caps));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
