use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

#[path = "support/cpuid_executable.rs"]
mod cpuid_executable;
#[path = "support/executable.rs"]
mod executable;

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn setup(prefix: &[u8], leaf: u32, subleaf: u32) -> (Cpu32, GuestMemory, u32) {
    let mut code = prefix.to_vec();
    code.extend_from_slice(&[0x0f, 0xa2]);
    let length = u32::try_from(code.len()).unwrap();
    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1000, &code).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    for (index, register) in REGISTERS.into_iter().enumerate() {
        cpu.set_register(
            register,
            0x1234_5678_u32.wrapping_mul(u32::try_from(index + 1).unwrap()),
        );
    }
    cpu.set_register(Register32::Eax, leaf);
    cpu.set_register(Register32::Ecx, subleaf);
    cpu.set_fs_base(0xdead_beef);
    cpu.set_x87_control_word(0x027f);
    (cpu, memory, length)
}

fn step(cpu: &mut Cpu32, memory: &mut GuestMemory) {
    let result = cpu.run(memory, 1);
    assert_eq!(result.reason, StopReason::InstructionLimit);
    assert_eq!(result.instructions, 1);
}

#[test]
fn software_vendor_order_and_identity_do_not_depend_on_id_or_ignored_prefixes() {
    for prefix in [
        vec![],
        vec![0x66],
        vec![0x67],
        vec![0x64],
        vec![0x64, 0x66, 0x67],
    ] {
        for flags in [2, 0x0020_0002, 0xced7, 0x0038_0002] {
            let (mut cpu, mut memory, length) = setup(&prefix, 0, 0xface_cafe);
            cpu.eflags = flags;
            let mut expected = cpu;
            expected.eip += length;
            expected.set_register(Register32::Eax, 1);
            expected.set_register(Register32::Ebx, u32::from_le_bytes(*b"Ring"));
            expected.set_register(Register32::Edx, u32::from_le_bytes(*b"3CPU"));
            expected.set_register(Register32::Ecx, u32::from_le_bytes(*b"Core"));
            step(&mut cpu, &mut memory);
            assert_eq!(cpu, expected);
            let vendor: Vec<_> = [Register32::Ebx, Register32::Edx, Register32::Ecx]
                .into_iter()
                .flat_map(|r| cpu.register(r).to_le_bytes())
                .collect();
            assert_eq!(vendor, b"Ring3CPUCore");
            assert_eq!(memory.mapped_pages(), 1);
        }
    }
}

#[test]
fn absent_features_extended_bound_and_out_of_range_fallback_are_deterministic() {
    for leaf in [
        1,
        2,
        0x21,
        0x7fff_ffff,
        0x4000_0000,
        0x4fff_ffff,
        0x8000_0000,
        0x8000_0001,
        u32::MAX,
    ] {
        for subleaf in [0, 1, 0xdead_beef, u32::MAX] {
            let (mut cpu, mut memory, length) = setup(&[], leaf, subleaf);
            cpu.eflags = 0xced7;
            let mut expected = cpu;
            expected.eip += length;
            for register in [
                Register32::Eax,
                Register32::Ebx,
                Register32::Ecx,
                Register32::Edx,
            ] {
                expected.set_register(register, 0);
            }
            if leaf == 0x8000_0000 {
                expected.set_register(Register32::Eax, 0x8000_0000);
            }
            let mut before = [0; 4096];
            memory.read(0x1000, &mut before).unwrap();
            step(&mut cpu, &mut memory);
            assert_eq!(cpu, expected);
            let mut after = [0; 4096];
            memory.read(0x1000, &mut after).unwrap();
            assert_eq!(after, before);
            assert_eq!(memory.mapped_pages(), 1);
        }
    }
}

#[test]
fn zero_budget_interception_and_refused_prefixes_preserve_query_state() {
    let (mut cpu, mut memory, _) = setup(&[], 0, 1);
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = cpu;
    assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
    let result = cpu.run_until(&mut memory, 1, |address| address == 0x1000);
    assert_eq!(result.reason, StopReason::Intercepted);
    assert_eq!(result.instructions, 0);
    assert_eq!(cpu, before);
    for prefix in [0xf0, 0xf2, 0xf3, 0x65] {
        let (mut cpu, mut memory, _) = setup(&[prefix], 0, 1);
        let before = cpu;
        let result = cpu.run(&mut memory, 1);
        assert!(matches!(
            result.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn authored_query_keeps_metadata_in_guest_memory_in_whole_and_single_step_runs() {
    use ring3_core::execution::{Process32, ProcessStop};
    let bytes = cpuid_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 0));
    for index in 0..15 {
        let result = stepped.run(1);
        assert_eq!((result.instructions, result.api_calls), (1, 0));
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(if index == 14 {
                StopReason::Breakpoint
            } else {
                StopReason::InstructionLimit
            })
        );
    }
    assert_eq!(stepped.cpu, whole.cpu);
    let mut expected = [0; 32];
    expected[0] = 1;
    expected[4..16].copy_from_slice(b"Ring3CPUCore");
    for process in [whole, stepped] {
        let mut bytes = [0xff; 32];
        process.memory.read(0x0040_2080, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(process.cpu.register(Register32::Eax), 0x8000_0000);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.cpu.eflags, 0x46);
    }
}
