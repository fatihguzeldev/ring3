use super::string_traversal_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn string_traversal_and_copy_match_native_whole_and_single_step_execution() {
    for (bytes, instructions, calls, eax, ebx, expected_output) in [
        (
            string_traversal_executable::increment(),
            8,
            2,
            0x0040_2182,
            0x0040_2181,
            [0; 8],
        ),
        (
            string_traversal_executable::copy(),
            6,
            1,
            0x0040_2190,
            0x0063_6261,
            [b'a', b'b', b'c', 0, 0x55, 0x55, 0x55, 0x55],
        ),
        (
            string_traversal_executable::terminated_copy(),
            5,
            1,
            0x0040_2190,
            0x6463_6261,
            [b'a', b'b', b'c', b'd', b'e', b'f', 0, 0x55],
        ),
        (
            string_traversal_executable::append(),
            5,
            1,
            0x0040_2190,
            0x6463_6261,
            [b'a', b'b', b'c', b'd', 0, 0x55, 0x55, 0x55],
        ),
    ] {
        let mut whole = Process32::load(&bytes, 25).unwrap();
        let mut stepped = Process32::load(&bytes, 25).unwrap();
        let result = whole.run(50);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(
            (result.instructions, result.api_calls),
            (instructions, calls)
        );
        let (mut steps, mut api_calls) = (0, 0);
        for _ in 0..50 {
            let step = stepped.run(1);
            steps += step.instructions;
            api_calls += step.api_calls;
            if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(step.reason, result.reason);
                break;
            }
        }
        assert_eq!((steps, api_calls), (instructions, calls));
        assert_eq!(whole.cpu, stepped.cpu);
        for process in [&whole, &stepped] {
            assert_eq!(process.cpu.register(Register32::Eax), eax);
            assert_eq!(process.cpu.register(Register32::Ebx), ebx);
            assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
            let mut output = [0; 8];
            process.memory.read(0x0040_2190, &mut output).unwrap();
            assert_eq!(output, expected_output);
        }
    }
}
