use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

fn setup(code: &[u8]) -> (Cpu32, GuestMemory) {
    let mut memory = GuestMemory::new(5);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1000, code).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    memory
        .map_zeroed(0x2000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Esi, 0x2001);
    cpu.set_register(Register32::Edi, 0x2ffd);
    cpu.eflags = 0xced7;
    (cpu, memory)
}

fn run(cpu: &mut Cpu32, memory: &mut GuestMemory, steps: u64) {
    assert_eq!(cpu.run(memory, steps).reason, StopReason::InstructionLimit);
    assert_eq!(cpu.eflags, 0xced7);
}

fn read_u64(memory: &GuestMemory, address: u64) -> u64 {
    let mut bytes = [0; 8];
    memory.read(address, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn transfers_preserve_bits_in_every_register_and_memory() {
    for register in 0..8 {
        let code = [
            0x0f,
            0x6f,
            0x06 | register << 3,
            0x0f,
            0x7f,
            0x07 | register << 3,
            0x0f,
            0x77,
        ];
        for bits in [
            0x7ff0_1234_5678_9abc_u64,
            0xfff8_0000_0000_0001,
            1,
            u64::MAX,
        ] {
            let (mut cpu, mut memory) = setup(&code);
            memory.write(0x2001, &bits.to_le_bytes()).unwrap();
            run(&mut cpu, &mut memory, 3);
            assert_eq!(read_u64(&memory, 0x2ffd), bits);
        }
    }
    let (mut cpu, mut memory) = setup(&[
        0x64, 0x0f, 0x6f, 0x06, 0x0f, 0x6f, 0xf8, 0x0f, 0x7f, 0xc7, 0x0f, 0x7f, 0x3f,
    ]);
    cpu.set_fs_base(0x1000);
    cpu.set_register(Register32::Esi, 0x1001);
    memory
        .write(0x2001, &0x0123_4567_89ab_cdef_u64.to_le_bytes())
        .unwrap();
    run(&mut cpu, &mut memory, 4);
    assert_eq!(read_u64(&memory, 0x2ffd), 0x0123_4567_89ab_cdef);
}

pub fn dword_moves_zero_extend_and_truncate() {
    let (mut cpu, mut memory) = setup(&[
        0x0f, 0x6f, 0x26, 0x0f, 0x7e, 0xe3, 0x0f, 0x6e, 0xe0, 0x0f, 0x7f, 0x27,
    ]);
    memory
        .write(0x2001, &0xfedc_ba98_7654_3210_u64.to_le_bytes())
        .unwrap();
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    run(&mut cpu, &mut memory, 4);
    assert_eq!(cpu.register(Register32::Ebx), 0x7654_3210);
    assert_eq!(read_u64(&memory, 0x2ffd), 0xabcd_1234);
    let (mut cpu, mut memory) = setup(&[0x0f, 0x6e, 0x3e, 0x0f, 0x7e, 0x3f]);
    memory.write(0x2001, &[0x12, 0x34, 0x56, 0x78]).unwrap();
    memory.write(0x2ffd, &[0xaa; 8]).unwrap();
    run(&mut cpu, &mut memory, 2);
    assert_eq!(read_u64(&memory, 0x2ffd), 0xaaaa_aaaa_7856_3412);
}

pub fn emms_preserves_physical_bits_status_and_top() {
    let (mut cpu, mut memory) = setup(&[
        0xd9, 0xe8, 0x0f, 0x77, 0xdf, 0xe0, 0x0f, 0x7f, 0x3f, 0xdf, 0xe0, 0x0f, 0x77, 0xd9, 0xe8,
        0xdd, 0x1f,
    ]);
    run(&mut cpu, &mut memory, 3);
    assert_eq!(cpu.register(Register32::Eax) & 0x3800, 0x3800);
    run(&mut cpu, &mut memory, 2);
    assert_eq!(read_u64(&memory, 0x2ffd), 0x8000_0000_0000_0000);
    assert_eq!(cpu.register(Register32::Eax) & 0x3800, 0);
    run(&mut cpu, &mut memory, 3);
    assert_eq!(read_u64(&memory, 0x2ffd), 1.0_f64.to_bits());
    let (mut cpu, mut memory) = setup(&[
        0xdd, 0x06, 0xdd, 0x1f, 0x0f, 0x6f, 0x06, 0x0f, 0x77, 0xdf, 0xe0, 0x0f, 0x7f, 0x07,
    ]);
    memory
        .write(0x2001, &0x7ff0_1234_5678_9abc_u64.to_le_bytes())
        .unwrap();
    run(&mut cpu, &mut memory, 6);
    assert_ne!(cpu.register(Register32::Eax) & 1, 0);
    assert_eq!(cpu.x87_control_word(), 0x027f);
    assert_eq!(read_u64(&memory, 0x2ffd), 0x7ff0_1234_5678_9abc);
}

pub fn faults_and_unsupported_numeric_transitions_are_atomic() {
    for operation in [0x6f, 0x7f] {
        for address in [0x3ffc, 0xffff_fffc] {
            let (mut cpu, mut memory) = setup(&[0x0f, 0x6f, 0x06, 0x0f, operation, 0x07]);
            memory.write(0x2001, &[0x5a; 8]).unwrap();
            memory.write(0x3ff8, &[0xa5; 8]).unwrap();
            run(&mut cpu, &mut memory, 1);
            cpu.set_register(Register32::Edi, address);
            let before = cpu;
            let prior = read_u64(&memory, 0x3ff8);
            assert!(matches!(
                cpu.run(&mut memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
            assert_eq!(read_u64(&memory, 0x3ff8), prior);
        }
    }
    let (mut cpu, mut memory) = setup(&[0x0f, 0x6f, 0x06, 0x0f, 0x7f, 0x07]);
    memory.write(0x2001, &[0x5a; 8]).unwrap();
    memory.write(0x2ffd, &[0xa5; 8]).unwrap();
    run(&mut cpu, &mut memory, 1);
    memory
        .protect(0x3000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    assert_eq!(read_u64(&memory, 0x2ffd), 0xa5a5_a5a5_a5a5_a5a5);
    for suffix in [
        &[0xdd, 0x1f][..],
        &[0xd9, 0xc0],
        &[0xd9, 0xe8],
        &[0xde, 0xc1],
    ] {
        let mut code = vec![0x0f, 0x6f, 0x06];
        code.extend_from_slice(suffix);
        let (mut cpu, mut memory) = setup(&code);
        run(&mut cpu, &mut memory, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read_u64(&memory, 0x2ffd), 0);
    }
    for code in [
        [0x66, 0x0f, 0x6f, 0x06],
        [0xf3, 0x0f, 0x6f, 0x06],
        [0x67, 0x0f, 0x6f, 0x06],
    ] {
        let (mut cpu, mut memory) = setup(&code);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}

pub fn physical_significands_survive_pops_and_pending_faults_stop_mmx() {
    for (bits, expected) in [
        (0_u64, 0_u64),
        (0x3ff8_0000_0000_0000, 0xc000_0000_0000_0000),
        (0xbff0_0000_0000_0000, 0x8000_0000_0000_0000),
        (1, 0x8000_0000_0000_0000),
        (3, 0xc000_0000_0000_0000),
        (0x7ff8_0000_0000_0001, 0xc000_0000_0000_0800),
    ] {
        let (mut cpu, mut memory) = setup(&[0xdd, 0x06, 0xdd, 0x1f, 0x0f, 0x7f, 0x3f]);
        memory.write(0x2001, &bits.to_le_bytes()).unwrap();
        run(&mut cpu, &mut memory, 3);
        assert_eq!(read_u64(&memory, 0x2ffd), expected);
    }
    for suffix in [&[0x0f, 0x77][..], &[0x0f, 0x6f, 0x06]] {
        let mut code = vec![0xdd, 0x06, 0xdd, 0x1f];
        code.extend_from_slice(suffix);
        let (mut cpu, mut memory) = setup(&code);
        memory
            .write(0x2001, &0x7ff0_0000_0000_0001_u64.to_le_bytes())
            .unwrap();
        run(&mut cpu, &mut memory, 2);
        cpu.set_x87_control_word(0x027e);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
