use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const OUTPUT: u32 = 0x0040_2a00;
const SOURCE_LOCK: u32 = OUTPUT + 0x10;
const DESTINATION_LOCK: u32 = OUTPUT + 0x20;

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn method(process: &Process32, object: u32, slot: u32) -> u32 {
    read(process, read(process, object) + slot * 4)
}

fn invoke(process: &mut Process32, address: u32, args: &[u32]) -> u32 {
    let mut frame = vec![0x0040_1000];
    frame.extend_from_slice(args);
    let bytes: Vec<_> = frame.into_iter().flat_map(u32::to_le_bytes).collect();
    process.memory.write(0x1000_fe00, &bytes).unwrap();
    process.cpu.set_register(Register32::Esp, 0x1000_fe00);
    process.cpu.eip = address;
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(
        process.cpu.register(Register32::Esp),
        0x1000_fe00 + u32::try_from(bytes.len()).unwrap()
    );
    process.cpu.register(Register32::Eax)
}

pub fn read_source_mip_while_writing_another() {
    for (format, pool) in [(21, 1), (21, 2), (22, 1), (22, 2)] {
        let mut process = Process32::load(&super::d3d8_executable::pe32(4, 3, 0), 32).unwrap();
        assert_eq!(
            process.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        let device = read(&process, 0x0040_2180);
        let create = method(&process, device, 20);
        assert_eq!(
            invoke(
                &mut process,
                create,
                &[device, 4, 4, 2, 0, format, pool, OUTPUT]
            ),
            0
        );
        let texture = read(&process, OUTPUT);
        let get_surface = method(&process, texture, 15);
        assert_eq!(invoke(&mut process, get_surface, &[texture, 0, OUTPUT]), 0);
        let source = read(&process, OUTPUT);
        assert_eq!(invoke(&mut process, get_surface, &[texture, 1, OUTPUT]), 0);
        let destination = read(&process, OUTPUT);
        let lock = method(&process, texture, 16);
        let unlock = method(&process, texture, 17);
        assert_eq!(
            invoke(&mut process, lock, &[texture, 0, SOURCE_LOCK, 0, 0]),
            0
        );
        let pixels = read(&process, SOURCE_LOCK + 4);
        let pattern: Vec<_> = (0..16_u32).flat_map(u32::to_le_bytes).collect();
        process.memory.write(u64::from(pixels), &pattern).unwrap();
        assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);
        let source_lock = method(&process, source, 9);
        let destination_lock = method(&process, destination, 9);
        let pages = process.memory.mapped_pages();
        assert_eq!(
            invoke(&mut process, source_lock, &[source, SOURCE_LOCK, 0, 0x10]),
            0
        );
        assert_eq!(
            invoke(
                &mut process,
                destination_lock,
                &[destination, DESTINATION_LOCK, 0, 0]
            ),
            0
        );
        assert_eq!(
            (
                read(&process, SOURCE_LOCK),
                read(&process, DESTINATION_LOCK)
            ),
            (16, 8)
        );
        assert_eq!(read(&process, SOURCE_LOCK + 4), pixels);
        let output = read(&process, DESTINATION_LOCK + 4);
        for (index, offset) in [0, 8, 32, 40].into_iter().enumerate() {
            let value = read(&process, pixels + offset);
            process
                .memory
                .write(
                    u64::from(output) + u64::try_from(index).unwrap() * 4,
                    &value.to_le_bytes(),
                )
                .unwrap();
        }
        assert_eq!(invoke(&mut process, unlock, &[texture, 1]), 0);
        assert_eq!(invoke(&mut process, unlock, &[texture, 0]), 0);
        assert_eq!(
            invoke(&mut process, lock, &[texture, 1, DESTINATION_LOCK, 0, 0x10]),
            0
        );
        assert_eq!(read(&process, DESTINATION_LOCK + 4), output);
        for (index, expected) in [0, 2, 8, 10].into_iter().enumerate() {
            assert_eq!(
                read(&process, output + u32::try_from(index).unwrap() * 4),
                expected
            );
        }
        assert_eq!(invoke(&mut process, unlock, &[texture, 1]), 0);
        let mut after = vec![0; 64];
        process.memory.read(u64::from(pixels), &mut after).unwrap();
        assert_eq!(after, pattern);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
}
