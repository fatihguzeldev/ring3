use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{CompileError, CompileLimits, RegistryError},
    memory::GuestAddress,
    process::{EngineInstance, ExecutionConfig32, HostError},
};

const KEY: u64 = 97;

fn config() -> ExecutionConfig32 {
    ExecutionConfig32 {
        units: 8,
        wasm_bytes: 1_048_576,
        promotion_hits: 2,
    }
}

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    let mut code = vec![0x90; 252];
    code.extend([0xd9, 0xe0].repeat(4));
    code.extend([0xff, 0xe2]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &code)
        .unwrap();
    engine
}

#[test]
fn selected_maximum_retains_256_instructions_and_late_group_coordinates() {
    assert_eq!(CompileLimits::default().instructions, 64);
    assert_eq!(CompileLimits::default().wasm_bytes, 65_536);
    for (instructions, wasm_bytes) in [(64, 65_536), (128, 131_072), (256, 262_144), (256, 524_288)]
    {
        let mut engine = fixture();
        let limits = CompileLimits {
            instructions,
            wasm_bytes,
            ..CompileLimits::default()
        };
        engine
            .configure_execution_with_limits(config(), limits)
            .unwrap();
        assert_eq!(engine.execution_config().unwrap(), Some(config()));
        assert_eq!(
            engine.execution_cache.as_ref().unwrap().compile_limits,
            limits
        );
        let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        let memory = engine.memory.as_ref().unwrap();
        let region = engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(memory, request.unit_id)
            .unwrap();
        assert_eq!(region.metadata().instructions, instructions);
        assert!(region.wasm_bytes(memory).unwrap().len() <= wasm_bytes);
        assert!(!region.contains_instruction(0x1104));
        if instructions == 256 {
            assert!(region.contains_instruction(0x1102));
            let group = region.x87_group(0).unwrap();
            assert_eq!(group.len(), 4);
            assert_eq!(group.steps()[0].pc, 0x10fc);
            assert_eq!(group.steps()[3].next_pc, 0x1104);
            assert!(region.x87_group(1).is_none());
        } else {
            assert!(!region.contains_instruction(0x10fc));
            assert!(region.x87_group(0).is_none());
        }
    }
}

#[test]
fn selected_maximum_does_not_change_manual_compilation_limits() {
    let mut engine = fixture();
    engine
        .configure_execution_with_limits(
            config(),
            CompileLimits {
                instructions: 256,
                wasm_bytes: 262_144,
                ..CompileLimits::default()
            },
        )
        .unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&0x1000u32.to_le_bytes());
    transfer[4..].copy_from_slice(&262u32.to_le_bytes());
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(
            CompileError::InstructionLimit
        )))
    );
    assert_eq!(engine.cache_usage().unwrap().units, 0);
}
