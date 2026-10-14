use super::support_fetch as fetch;
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        ControlOperation,
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

#[test]
fn sysenter_is_a_distinct_memory_free_terminal_control() {
    for prefix in [
        &[][..],
        &[0x66],
        &[0x67],
        &[0x26],
        &[0x2e],
        &[0x36],
        &[0x3e],
        &[0x64],
        &[0x65],
    ] {
        let bytes = [prefix, &[0x0f, 0x34]].concat();
        let memory = memory(0x1000, &bytes);
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::Sysenter)
        );
        assert!(!ControlOperation::Sysenter.reads_memory());
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (bytes.len(), GuestAddress(0x1000 + bytes.len() as u32))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn sysenter_bound_compilers_stop_before_invalid_tail_without_stack_or_kernel_tables() {
    for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut engine = EngineInstance::new(1, 0x5359_5345_4e54_4552).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&[0x0f, 0x34, 0xf0, 0x90]);
        engine.upload(0x1000, 4).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0x1000_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat());
        let before = engine.arena().to_vec();
        let mut ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut ram)
            .unwrap();
        let id = match (resident, entries) {
            (false, false) => engine.compile(1).map(|_| None),
            (false, true) => engine.compile_entries(1, 0).map(|_| None),
            (true, false) => engine.compile_resident(1).map(|id| Some(id.get())),
            (true, true) => engine
                .compile_resident_entries(1, 0)
                .map(|id| Some(id.get())),
        }
        .unwrap();
        let bytes = id.map_or_else(
            || engine.artifact_bytes().unwrap(),
            |id| engine.resident_bytes(id).unwrap(),
        );
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
        assert_eq!(engine.arena(), before);
        let mut after = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut after)
            .unwrap();
        assert_eq!(after, ram);
    }
}

#[test]
fn sysenter_fetch_and_reserved_prefix_failures_precede_entry_admission() {
    fetch::assert_error(
        decode_one(&memory(0x1fff, &[0x0f]), GuestAddress(0x1fff)).unwrap_err(),
        0x1fff,
        0x2000,
        FaultReason::Unmapped,
        2,
    );
    for bytes in [
        &[0xf2, 0x0f, 0x34][..],
        &[0xf3, 0x0f, 0x34],
        &[0x66, 0x66, 0x0f, 0x34],
    ] {
        assert!(matches!(
            decode_one(&memory(0x1000, bytes), GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
    assert_eq!(
        decode_one(&memory(0x1000, &[0xf0, 0x0f, 0x34]), GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    assert!(matches!(
        decode_one(&memory(0x1000, &[0x0f, 0x35]), GuestAddress(0x1000)),
        Err(DecodeError::Unsupported(_))
    ));
}

#[test]
fn standalone_sysenter_decode_does_not_invent_an_os_entry_owner() {
    use ring3_engine::cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
        compile_region,
    };
    let memory = memory(0x1000, &[0x0f, 0x34]);
    let expected = CompileError::Instruction {
        pc: GuestAddress(0x1000),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 2
            }],
            CompileLimits::default()
        )
        .unwrap_err(),
        expected
    );
    assert_eq!(
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
            .unwrap_err(),
        expected
    );
}
