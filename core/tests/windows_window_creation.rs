#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn words(p: &Process32, address: u32, count: usize) -> Vec<u32> {
    let mut bytes = vec![0; count * 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

#[test]
fn hidden_creation_delivers_ordered_guest_messages_and_returns_a_real_window() {
    let mut p = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    let mut messages = Vec::new();
    let mut counts = (0, 0);
    loop {
        if p.cpu.eip == 0x7000_02ac {
            let frame = words(&p, p.cpu.register(Register32::Esp), 5);
            assert_eq!(frame[1], 0x7500_0004);
            assert_eq!(frame[3], 0);
            messages.push(frame[2]);
            match frame[2] {
                0x81 | 1 => assert_eq!(
                    words(&p, frame[4], 12),
                    [
                        0x1234,
                        0x0040_0000,
                        0,
                        0,
                        90,
                        130,
                        20,
                        10,
                        0x00ca_0000,
                        0x0040_2190,
                        0x0040_2180,
                        0
                    ]
                ),
                0x83 => assert_eq!(words(&p, frame[4], 4), [10, 20, 140, 110]),
                0x24 => assert_eq!(
                    words(&p, frame[4], 10),
                    [
                        0,
                        0,
                        646,
                        486,
                        (-3_i32).cast_unsigned(),
                        (-3_i32).cast_unsigned(),
                        112,
                        27,
                        652,
                        492
                    ]
                ),
                _ => panic!("unexpected creation message"),
            }
        }
        let run = p.run(1);
        counts.0 += run.instructions;
        counts.1 += run.api_calls;
        if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            break;
        }
        assert!(counts.0 + counts.1 < 200);
    }
    assert_eq!(messages, [0x24, 0x81, 0x83, 1]);
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    let mut whole = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    let run = whole.run(200);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), counts);
    assert_eq!(whole.cpu, p.cpu);
}
