use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const OUTPUT: u32 = 0x0040_2e00;
const INVALID_CALL: u32 = 0x8876_086c;

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn write(process: &mut Process32, address: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    process.memory.write(u64::from(address), &bytes).unwrap();
}

fn invoke(process: &mut Process32, device: u32, slot: u32, args: &[u32]) -> u32 {
    let target = read(process, read(process, device) + slot * 4);
    let mut frame = vec![0x0040_1000, device];
    frame.extend_from_slice(args);
    write(process, 0x1000_fe00, &frame);
    process.cpu.set_register(Register32::Esp, 0x1000_fe00);
    process.cpu.eip = target;
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(
        process.cpu.register(Register32::Esp),
        0x1000_fe00 + u32::try_from(frame.len()).unwrap() * 4
    );
    process.cpu.register(Register32::Eax)
}

fn setup(fvf: u32, stride: u32, index32: bool, truncate: u32) -> (Process32, u32, u32) {
    let mut bytes = super::d3d8_executable::pe32(4, 3, 0xff00_0000);
    bytes[1312..1316].copy_from_slice(&1_u32.to_le_bytes());
    bytes[1316..1320].copy_from_slice(&80_u32.to_le_bytes());
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    process.take_frame().unwrap();
    let device = read(&process, 0x0040_2180);
    let size = if fvf == 0x152 { 36 } else { 24 };
    assert_eq!(
        invoke(
            &mut process,
            device,
            23,
            &[5 * stride + size - truncate, 0, fvf, 0, OUTPUT]
        ),
        0
    );
    let vertex = read(&process, OUTPUT);
    for (index, (x, y, u, v)) in [
        (-1_f32, 1_f32, 0.1_f32, 0.1_f32),
        (1.0, 1.0, 0.9, 0.1),
        (-1.0, -1.0, 0.1, 0.9),
        (-1.0, -1.0, 0.1, 0.9),
    ]
    .into_iter()
    .enumerate()
    {
        let mut words = vec![x.to_bits(), y.to_bits(), 0.25_f32.to_bits()];
        if fvf == 0x152 {
            // unused normal bits must not become color or texture coordinates.
            words.extend_from_slice(&[
                f32::NAN.to_bits(),
                f32::INFINITY.to_bits(),
                (-1_f32).to_bits(),
            ]);
        }
        words.extend_from_slice(&[0xff80_ffff, u.to_bits(), v.to_bits()]);
        write(
            &mut process,
            vertex + 4096 + (u32::try_from(index).unwrap() + 2) * stride,
            &words,
        );
    }
    let indices: Vec<_> = [99_u32, 1, 2, 3, 1, 2, 4]
        .into_iter()
        .flat_map(|index| {
            let bytes = index.to_le_bytes();
            bytes[..if index32 { 4 } else { 2 }].to_vec()
        })
        .collect();
    assert_eq!(
        invoke(
            &mut process,
            device,
            24,
            &[
                u32::try_from(indices.len()).unwrap(),
                0,
                if index32 { 102 } else { 101 },
                0,
                OUTPUT
            ]
        ),
        0
    );
    let index = read(&process, OUTPUT);
    process
        .memory
        .write(u64::from(index + 4096), &indices)
        .unwrap();
    assert_eq!(invoke(&mut process, device, 76, &[fvf]), 0);
    assert_eq!(invoke(&mut process, device, 83, &[0, vertex, stride]), 0);
    assert_eq!(invoke(&mut process, device, 85, &[index, 1]), 0);
    assert_eq!(
        invoke(
            &mut process,
            device,
            36,
            &[0, 0, 3, 0xff00_0000, 1_f32.to_bits(), 0]
        ),
        0
    );
    (process, device, vertex)
}

fn frame(process: &mut Process32, device: u32) -> Vec<u8> {
    assert_eq!(invoke(process, device, 15, &[0, 0, 0, 0]), 0);
    process.take_frame().unwrap().rgba
}

fn draw(process: &mut Process32, device: u32) -> u32 {
    invoke(process, device, 71, &[4, 1, 4, 1, 2])
}

pub fn equivalent_diffuse_and_textured_pixels() {
    let mut expected = None;
    for index32 in [false, true] {
        for (fvf, stride) in [(0x142, 24), (0x152, 36), (0x152, 48)] {
            let (mut process, device, _) = setup(fvf, stride, index32, 0);
            assert_eq!(draw(&mut process, device), 0);
            let solid = frame(&mut process, device);
            assert_eq!(&solid[..4], &[128, 255, 255, 255]);
            assert_eq!(&solid[44..48], &[0, 0, 0, 255]);
            assert_eq!(
                invoke(&mut process, device, 20, &[2, 2, 1, 0, 21, 1, OUTPUT]),
                0
            );
            let texture = read(&process, OUTPUT);
            write(
                &mut process,
                texture + 4096,
                &[0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xffff_ffff],
            );
            assert_eq!(invoke(&mut process, device, 61, &[0, texture]), 0);
            assert_eq!(draw(&mut process, device), 0);
            let textured = frame(&mut process, device);
            assert_eq!(&textured[..4], &[128, 0, 0, 255]);
            assert_eq!(&textured[8..12], &[0, 255, 0, 255]);
            assert_eq!(&textured[32..36], &[0, 0, 255, 255]);
            if let Some((expected_solid, expected_textured)) = &expected {
                assert_eq!(&solid, expected_solid);
                assert_eq!(&textured, expected_textured);
            } else {
                expected = Some((solid, textured));
            }
        }
    }
}

pub fn rejects_short_records_and_nonfinite_inputs_atomically() {
    for bad_offset in [0, 4, 8, 28, 32] {
        let (mut process, device, vertex) = setup(0x152, 36, false, 0);
        let last = vertex + 4096 + 5 * 36 + bad_offset;
        let original = read(&process, last);
        write(&mut process, last, &[f32::NAN.to_bits()]);
        assert_eq!(draw(&mut process, device), INVALID_CALL);
        assert!(
            frame(&mut process, device)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
        write(&mut process, last, &[original]);
        for index in 2..6 {
            write(
                &mut process,
                vertex + 4096 + index * 36 + 8,
                &[0.75_f32.to_bits()],
            );
        }
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
    for stride in [36, 48] {
        let (mut process, device, vertex) = setup(0x152, stride, true, 1);
        assert_eq!(draw(&mut process, device), INVALID_CALL);
        assert!(
            frame(&mut process, device)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
        for index in 2..5 {
            write(
                &mut process,
                vertex + 4096 + index * stride + 8,
                &[0.75_f32.to_bits()],
            );
        }
        assert_eq!(invoke(&mut process, device, 71, &[4, 1, 3, 1, 1]), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
    let (mut process, device, vertex) = setup(0x152, 36, false, 0);
    for stride in [24, 35] {
        assert_eq!(invoke(&mut process, device, 83, &[0, vertex, stride]), 0);
        assert_eq!(draw(&mut process, device), INVALID_CALL);
        assert!(
            frame(&mut process, device)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
    }
    assert_eq!(invoke(&mut process, device, 83, &[0, vertex, 36]), 0);
    assert_eq!(draw(&mut process, device), 0);
    assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
}

pub fn rejected_selection_preserves_the_normal_layout() {
    let (mut process, device, _) = setup(0x152, 36, false, 0);
    for fvf in [0x112, 0x156, 0x1d2, 0x252, 0x10152, u32::MAX] {
        assert_eq!(invoke(&mut process, device, 76, &[fvf]), INVALID_CALL);
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
    assert_eq!(
        invoke(&mut process, device, 72, &[4, 0, 0, 36]),
        INVALID_CALL
    );
    assert_eq!(invoke(&mut process, device, 76, &[0x44]), 0);
    assert_eq!(draw(&mut process, device), INVALID_CALL);
    assert_eq!(invoke(&mut process, device, 76, &[0x152]), 0);
    assert_eq!(draw(&mut process, device), 0);
}
