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

#[expect(clippy::too_many_lines, reason = "shared indexed scene fixture")]
fn setup(fvf: u32, stride: u32, index32: bool, truncate: u32) -> (Process32, u32, u32) {
    let mut bytes = super::d3d8_executable::pe32(4, 3, 0xff00_0000);
    bytes[1312..1316].copy_from_slice(&1_u32.to_le_bytes());
    bytes[1316..1320].copy_from_slice(&80_u32.to_le_bytes());
    let mut process = Process32::load(&bytes, 40).unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    process.take_frame().unwrap();
    let device = read(&process, 0x0040_2180);
    let (size, normal, diffuse, sets) = match fvf {
        0x102 => (20, false, false, 1),
        0x112 => (32, true, false, 1),
        0x142 => (24, false, true, 1),
        0x152 => (36, true, true, 1),
        0x202 => (28, false, false, 2),
        0x212 => (40, true, false, 2),
        0x242 => (32, false, true, 2),
        0x252 => (44, true, true, 2),
        _ => panic!("fixture layout"),
    };
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
        if normal {
            // unused normal bits must not become color or texture coordinates.
            words.extend_from_slice(&[
                f32::NAN.to_bits(),
                f32::INFINITY.to_bits(),
                (-1_f32).to_bits(),
            ]);
        }
        if diffuse {
            words.push(0xff80_ffff);
        }
        words.extend_from_slice(&[u.to_bits(), v.to_bits()]);
        if sets == 2 {
            words.extend_from_slice(&[(1.0 - u).to_bits(), (1.0 - v).to_bits()]);
        }
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
    for fvf in [0x312, 0x156, 0x1d2, 0x256, 0x10152, u32::MAX] {
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

fn translate_z(process: &mut Process32, device: u32, state: u32, z: f32) {
    let mut matrix = [0; 16];
    for index in [0, 5, 10, 15] {
        matrix[index] = 1_f32.to_bits();
    }
    matrix[14] = z.to_bits();
    write(process, OUTPUT + 64, &matrix);
    assert_eq!(invoke(process, device, 37, &[state, OUTPUT + 64]), 0);
}

pub fn world_transform_controls_positions() {
    for (fvf, stride) in [(0x142, 24), (0x152, 36)] {
        let (mut process, device, _) = setup(fvf, stride, false, 0);
        translate_z(&mut process, device, 257, 1.0);
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
        assert_eq!(
            invoke(
                &mut process,
                device,
                36,
                &[0, 0, 3, 0xff00_0000, 1_f32.to_bits(), 0]
            ),
            0
        );
        translate_z(&mut process, device, 256, 1.0);
        assert_eq!(draw(&mut process, device), 0);
        assert!(
            frame(&mut process, device)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
        translate_z(&mut process, device, 256, 0.0);
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
}

pub fn texture_matrix_does_not_substitute_for_world() {
    for (fvf, stride) in [(0x142, 24), (0x152, 36)] {
        let (mut process, device, _) = setup(fvf, stride, false, 0);
        translate_z(&mut process, device, 16, 1.0);
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
        assert_eq!(
            invoke(
                &mut process,
                device,
                36,
                &[0, 0, 3, 0xff00_0000, 1_f32.to_bits(), 0]
            ),
            0
        );
        translate_z(&mut process, device, 256, 1.0);
        translate_z(&mut process, device, 16, -1.0);
        assert_eq!(draw(&mut process, device), 0);
        assert!(
            frame(&mut process, device)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
    }
}

fn draw_at_depth(process: &mut Process32, device: u32, vertex: u32, stride: u32, z: f32, up: bool) {
    if up {
        let mut words = Vec::new();
        for (x, y) in [(0_f32, 0_f32), (4.0, 0.0), (0.0, 3.0)] {
            words.extend_from_slice(&[
                x.to_bits(),
                y.to_bits(),
                z.to_bits(),
                1_f32.to_bits(),
                0xff80_ffff,
            ]);
        }
        write(process, OUTPUT + 128, &words);
        assert_eq!(invoke(process, device, 76, &[0x44]), 0);
        assert_eq!(invoke(process, device, 72, &[4, 1, OUTPUT + 128, 20]), 0);
    } else {
        for index in 2..6 {
            write(process, vertex + 4096 + index * stride + 8, &[z.to_bits()]);
        }
        assert_eq!(draw(process, device), 0);
    }
}

fn clear_depth_case(process: &mut Process32, device: u32, flags: u32, z: f32) {
    assert_eq!(
        invoke(
            process,
            device,
            36,
            &[0, 0, flags, 0xff00_0000, z.to_bits(), 0]
        ),
        0
    );
}

pub fn depth_comparison_functions_match_d16_values() {
    let expected = [
        [false, false, false],
        [true, false, false],
        [false, true, false],
        [true, true, false],
        [false, false, true],
        [true, false, true],
        [false, true, true],
        [true, true, true],
    ];
    for (fvf, stride, up) in [(0x142, 24, false), (0x152, 36, false), (0x142, 24, true)] {
        let (mut process, device, vertex) = setup(fvf, stride, false, 0);
        assert_eq!(invoke(&mut process, device, 50, &[14, 0]), 0);
        assert_eq!(read(&process, 0x0040_2828), 0xff);
        assert_eq!(read(&process, 0x0040_2820) & 2, 2);
        for (index, outcomes) in expected.iter().enumerate() {
            let function = u32::try_from(index + 1).unwrap();
            assert_eq!(invoke(&mut process, device, 50, &[23, function]), 0);
            for (z, pass) in [0.25_f32, 0.5, 0.75].into_iter().zip(outcomes) {
                clear_depth_case(&mut process, device, 3, 0.5);
                draw_at_depth(&mut process, device, vertex, stride, z, up);
                let expected = if *pass {
                    [128, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(
                    &frame(&mut process, device)[..4],
                    &expected,
                    "fvf={fvf:x} up={up} function={function} z={z}"
                );
            }
        }
        assert_eq!(invoke(&mut process, device, 50, &[23, 3]), 0);
        clear_depth_case(&mut process, device, 3, 0.5);
        draw_at_depth(&mut process, device, vertex, stride, 0.500_000_1, up);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
}

pub fn depth_writes_are_independent_of_tests_and_clear() {
    for (fvf, stride, up) in [(0x142, 24, false), (0x152, 36, false), (0x142, 24, true)] {
        let (mut process, device, vertex) = setup(fvf, stride, false, 0);
        assert_eq!(invoke(&mut process, device, 50, &[14, 0]), 0);
        draw_at_depth(&mut process, device, vertex, stride, 0.25, up);
        clear_depth_case(&mut process, device, 1, 0.0);
        draw_at_depth(&mut process, device, vertex, stride, 0.75, up);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
        assert_eq!(invoke(&mut process, device, 50, &[14, 1]), 0);
        draw_at_depth(&mut process, device, vertex, stride, 0.25, up);
        assert_eq!(invoke(&mut process, device, 50, &[14, 0]), 0);
        clear_depth_case(&mut process, device, 1, 0.0);
        draw_at_depth(&mut process, device, vertex, stride, 0.75, up);
        assert_eq!(&frame(&mut process, device)[..4], &[0, 0, 0, 255]);

        assert_eq!(invoke(&mut process, device, 50, &[7, 0]), 0);
        assert_eq!(invoke(&mut process, device, 50, &[14, 1]), 0);
        assert_eq!(invoke(&mut process, device, 50, &[23, 1]), 0);
        draw_at_depth(&mut process, device, vertex, stride, 0.75, up);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
        assert_eq!(invoke(&mut process, device, 50, &[7, 1]), 0);
        assert_eq!(invoke(&mut process, device, 50, &[23, 4]), 0);
        clear_depth_case(&mut process, device, 1, 0.0);
        draw_at_depth(&mut process, device, vertex, stride, 0.5, up);
        assert_eq!(&frame(&mut process, device)[..4], &[0, 0, 0, 255]);

        assert_eq!(invoke(&mut process, device, 50, &[14, 0]), 0);
        assert_eq!(invoke(&mut process, device, 50, &[23, 1]), 0);
        clear_depth_case(&mut process, device, 3, 1.0);
        assert_eq!(invoke(&mut process, device, 50, &[23, 4]), 0);
        draw_at_depth(&mut process, device, vertex, stride, 0.75, up);
        assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    }
}

pub fn invalid_depth_policy_preserves_the_previous_state() {
    let (mut process, device, vertex) = setup(0x152, 36, false, 0);
    clear_depth_case(&mut process, device, 3, 0.5);
    assert_eq!(invoke(&mut process, device, 50, &[14, 0]), 0);
    assert_eq!(invoke(&mut process, device, 50, &[23, 8]), 0);
    for (state, value) in [(14, 2), (14, u32::MAX), (23, 0), (23, 9), (23, u32::MAX)] {
        assert_eq!(
            invoke(&mut process, device, 50, &[state, value]),
            INVALID_CALL
        );
    }
    draw_at_depth(&mut process, device, vertex, 36, 0.75, false);
    assert_eq!(&frame(&mut process, device)[..4], &[128, 255, 255, 255]);
    assert_eq!(invoke(&mut process, device, 50, &[23, 2]), 0);
    clear_depth_case(&mut process, device, 1, 0.0);
    draw_at_depth(&mut process, device, vertex, 36, 0.6, false);
    assert_eq!(&frame(&mut process, device)[..4], &[0, 0, 0, 255]);
}

pub fn vertex_storage_accepts_texture_coordinate_counts() {
    let (mut process, device, _) = setup(0x142, 24, false, 0);
    let pages = process.memory.mapped_pages();
    for count in 0..=8 {
        for pool in 0..=2 {
            let fvf = 0x12 | (count << 8);
            assert_eq!(
                invoke(&mut process, device, 23, &[5760, 0x18, fvf, pool, OUTPUT]),
                0
            );
            let buffer = read(&process, OUTPUT);
            let mut bytes = vec![1; 5760];
            process
                .memory
                .read(u64::from(buffer + 4096), &mut bytes)
                .unwrap();
            assert!(bytes.iter().all(|byte| *byte == 0));
            assert_eq!(invoke(&mut process, buffer, 11, &[0, 5760, OUTPUT, 0]), 0);
            let data = read(&process, OUTPUT);
            process
                .memory
                .write(u64::from(data + 5756), &[1, 2, 3, 4])
                .unwrap();
            assert_eq!(invoke(&mut process, buffer, 12, &[]), 0);
            assert_eq!(invoke(&mut process, buffer, 2, &[]), 0);
            assert_eq!(process.memory.mapped_pages(), pages);
        }
    }
    for fvf in [0x912, 0xf12, 0x214, 0x0001_0212, 0x213] {
        write(&mut process, OUTPUT, &[0x1234_5678]);
        assert_eq!(
            invoke(&mut process, device, 23, &[5760, 0x18, fvf, 0, OUTPUT]),
            INVALID_CALL
        );
        assert_eq!(read(&process, OUTPUT), 0x1234_5678);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    assert_eq!(invoke(&mut process, device, 76, &[0x912]), INVALID_CALL);
    assert_eq!(draw(&mut process, device), 0);
}

fn stage(process: &mut Process32, device: u32, index: u32, kind: u32, value: u32) {
    assert_eq!(invoke(process, device, 63, &[index, kind, value]), 0);
    assert_eq!(invoke(process, device, 62, &[index, kind, OUTPUT]), 0);
    assert_eq!(read(process, OUTPUT), value);
}

fn texture(process: &mut Process32, device: u32, width: u32, height: u32, pixels: &[u32]) -> u32 {
    assert_eq!(
        invoke(process, device, 20, &[width, height, 1, 0, 21, 1, OUTPUT]),
        0
    );
    let texture = read(process, OUTPUT);
    write(process, texture + 4096, pixels);
    texture
}

pub fn two_texture_color_stages() {
    for (fvf, stride, diffuse) in [
        (0x102, 20, 255),
        (0x112, 32, 255),
        (0x202, 28, 255),
        (0x212, 40, 255),
        (0x242, 32, 128),
        (0x252, 44, 128),
    ] {
        let (mut process, device, _) = setup(fvf, stride, false, 0);
        assert_eq!(draw(&mut process, device), 0);
        assert_eq!(&frame(&mut process, device)[..4], &[diffuse, 255, 255, 255]);
    }
    for index32 in [false, true] {
        let (mut p, d, _) = setup(0x212, 40, index32, 0);
        let base = texture(
            &mut p,
            d,
            2,
            2,
            &[0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xffff_ffff],
        );
        let light = texture(
            &mut p,
            d,
            2,
            2,
            &[0xff20_ff40, 0xff40_80ff, 0xff60_4000, 0xff80_40ff],
        );
        assert_eq!(invoke(&mut p, d, 61, &[0, base]), 0);
        assert_eq!(invoke(&mut p, d, 61, &[1, light]), 0);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 0, 0, 255]);
        stage(&mut p, d, 1, 1, 4);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[128, 0, 0, 255]);
        stage(&mut p, d, 1, 11, 0);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[32, 0, 0, 255]);
        stage(&mut p, d, 1, 11, 1);
        stage(&mut p, d, 1, 1, 5);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 0, 0, 255]);
        stage(&mut p, d, 1, 1, 2);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[128, 64, 255, 255]);
        stage(&mut p, d, 1, 1, 3);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 0, 0, 255]);
        stage(&mut p, d, 1, 3, 2);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[128, 64, 255, 255]);
        stage(&mut p, d, 1, 3, 1);
        stage(&mut p, d, 1, 2, 0);
        stage(&mut p, d, 1, 1, 2);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 255, 255, 255]);
        stage(&mut p, d, 0, 1, 1);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 255, 255, 255]);
        stage(&mut p, d, 0, 1, 4);
        assert_eq!(invoke(&mut p, d, 61, &[0, 0]), 0);
        assert_eq!(draw(&mut p, d), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 255, 255, 255]);
    }
    let (mut p, d, _) = setup(0x212, 40, false, 0);
    let vertices: Vec<_> = [(0_f32, 0_f32), (4.0, 0.0), (0.0, 3.0)]
        .into_iter()
        .flat_map(|(x, y)| [x.to_bits(), y.to_bits(), 0, 1_f32.to_bits(), 0xffff_ffff])
        .collect();
    write(&mut p, OUTPUT, &vertices);
    assert_eq!(invoke(&mut p, d, 76, &[0x44]), 0);
    // custom color stages are supported only by the indexed path.
    stage(&mut p, d, 0, 1, 2);
    assert_eq!(invoke(&mut p, d, 72, &[4, 1, OUTPUT, 20]), INVALID_CALL);
    assert_eq!(&frame(&mut p, d)[..4], &[0, 0, 0, 255]);
    stage(&mut p, d, 0, 1, 4);
    write(&mut p, OUTPUT, &vertices);
    assert_eq!(invoke(&mut p, d, 72, &[4, 1, OUTPUT, 20]), 0);
    assert_eq!(&frame(&mut p, d)[..4], &[255, 255, 255, 255]);
}

pub fn two_texture_perspective_and_clipping() {
    for first_x in [-0.5_f32, -1.0] {
        let (mut p, d, vertex) = setup(0x212, 40, false, 0);
        for (index, (x, y, z, u)) in [
            (first_x, 0.5_f32, 0.5_f32, 0_f32),
            (1.0, 1.0, 1.0, 1.0),
            (-1.0, -1.0, 1.0, 0.0),
            (-1.0, -1.0, 1.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let offset = vertex + 4096 + (u32::try_from(index).unwrap() + 2) * 40;
            write(&mut p, offset, &[x.to_bits(), y.to_bits(), z.to_bits()]);
            write(&mut p, offset + 32, &[u.to_bits(), 0_f32.to_bits()]);
        }
        let projection = [
            1_f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 1., 0., 0., 0., 0.,
        ];
        write(&mut p, OUTPUT, &projection.map(f32::to_bits));
        assert_eq!(invoke(&mut p, d, 37, &[3, OUTPUT]), 0);
        let gradient: Vec<_> = (0..8).map(|i| 0xff00_0000 | ((i * 20) << 16)).collect();
        let t = texture(&mut p, d, 8, 1, &gradient);
        assert_eq!(invoke(&mut p, d, 61, &[1, t]), 0);
        stage(&mut p, d, 0, 2, 0);
        stage(&mut p, d, 0, 1, 3);
        stage(&mut p, d, 1, 1, 2);
        assert_eq!(draw(&mut p, d), 0);
        // perspective-correct u is 15/29 or 25/41 here; affine u selects texel five.
        assert_eq!(&frame(&mut p, d)[8..12], &[80, 0, 0, 255]);
    }
}

pub fn two_texture_failures_preserve_pixels_and_depth() {
    for (stride, truncate, bad_offset) in [(40, 0, Some(32)), (56, 0, Some(36)), (40, 1, None)] {
        let (mut p, d, vertex) = setup(0x212, stride, true, truncate);
        let original = bad_offset.map(|offset| read(&p, vertex + 4096 + 5 * stride + offset));
        if let Some(offset) = bad_offset {
            write(
                &mut p,
                vertex + 4096 + 5 * stride + offset,
                &[f32::NAN.to_bits()],
            );
        }
        assert_eq!(draw(&mut p, d), INVALID_CALL);
        assert!(
            frame(&mut p, d)
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
        if let (Some(offset), Some(value)) = (bad_offset, original) {
            write(&mut p, vertex + 4096 + 5 * stride + offset, &[value]);
        }
        for index in 2..5 {
            write(
                &mut p,
                vertex + 4096 + index * stride + 8,
                &[0.75_f32.to_bits()],
            );
        }
        assert_eq!(invoke(&mut p, d, 71, &[4, 1, 3, 1, 1]), 0);
        assert_eq!(&frame(&mut p, d)[..4], &[255, 255, 255, 255]);
    }
    let (mut p, d, _) = setup(0x212, 40, false, 0);
    for (index, kind, value) in [(0, 1, 6), (2, 1, 4), (0, 2, 3), (0, 11, 8), (8, 1, 1)] {
        assert_eq!(invoke(&mut p, d, 63, &[index, kind, value]), INVALID_CALL);
    }
    assert_eq!(draw(&mut p, d), 0);
    let t = texture(&mut p, d, 1, 1, &[0xff00_ff00]);
    assert_eq!(invoke(&mut p, d, 61, &[0, t]), 0);
    stage(&mut p, d, 0, 11, 2);
    assert_eq!(draw(&mut p, d), INVALID_CALL);
    stage(&mut p, d, 0, 11, 0);
    assert_eq!(draw(&mut p, d), 0);
    assert_eq!(&frame(&mut p, d)[..4], &[0, 255, 0, 255]);
}
