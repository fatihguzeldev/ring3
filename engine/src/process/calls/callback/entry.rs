impl crate::process::EngineInstance {
    pub(in crate::process::calls) fn compiled_callback_entry(
        &self,
        artifact: &crate::cpu::dbt::CompiledRegion,
        pc: u32,
    ) -> bool {
        artifact.contains_instruction(pc)
            || self
                .module_gate_at(pc)
                .is_some_and(|gate| artifact.matches_gate(pc, gate.id))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        abi::arena::TRANSFER_OFFSET,
        memory::GuestAddress,
        process::{CallError, EngineInstance, HostError},
        windows::{CallingConvention32, WindowsApi32},
    };
    const KEY: u64 = 91;
    const OUTER: u32 = 0x10000;
    const RETURN: u32 = 0x10020;
    const FAKE: u32 = 0x10040;
    const OUTER_ID: u32 = 123;
    const RETURN_ID: u32 = 124;
    fn stop(engine: &mut EngineInstance, pc: u32, gate: u32, esp: u32, eax: u32) {
        use crate::{
            abi::{arena::EXIT_OFFSET, x86::*},
            cpu::{ExecutionExit, ExitReason},
        };
        let mut state = crate::cpu::x86::State32 {
            eip: pc,
            ..Default::default()
        };
        state.registers[0] = eax;
        state.registers[4] = esp;
        encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::Gate { id: gate },
            },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
    }

    fn engine() -> (EngineInstance, u32) {
        let mut engine = EngineInstance::new(128, KEY).unwrap();
        engine.register_kernel32_module(0x70000000).unwrap();
        engine
            .register_guest_module(
                "callback-entry.exe",
                &crate::process::windows::process::test_image_for_startup(),
            )
            .unwrap();
        engine
            .start_module_process("callback-entry.exe", None, Default::default())
            .unwrap();
        engine.load_library_module("kernel32.dll", None).unwrap();
        let entry = engine
            .module_gates()
            .unwrap()
            .into_iter()
            .find(|gate| gate.id == WindowsApi32::GetLastError.id())
            .unwrap()
            .entry
            .0;
        engine.map(OUTER, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        for address in [OUTER, RETURN, FAKE] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(address), &[0x0f, 0x0b])
                .unwrap();
        }
        engine.write32(0x8100, 0x2000).unwrap();
        (engine, entry)
    }
    fn descriptors(engine: &mut EngineInstance, entries: &[(u32, u32)]) {
        for (index, &(pc, id)) in entries.iter().enumerate() {
            for (offset, value) in [
                (0, pc),
                (4, 2),
                (entries.len() * 8, pc),
                (entries.len() * 8 + 4, id),
            ] {
                let at = TRANSFER_OFFSET + index * 8 + offset;
                engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    #[test]
    fn canonical_loaded_gate_callback_entries_are_admitted_in_resident_and_replacement_modes() {
        for resident in [true, false] {
            let (mut engine, entry) = engine();
            let gates = [
                (OUTER, OUTER_ID),
                (entry, WindowsApi32::GetLastError.id()),
                (RETURN, RETURN_ID),
            ];
            descriptors(&mut engine, &gates);
            if resident {
                descriptors(&mut engine, &[(OUTER, OUTER_ID)]);
                let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
                descriptors(&mut engine, &gates[1..]);
                let unit = engine.compile_resident_with_gates(2, 2).unwrap().get();
                stop(&mut engine, OUTER, OUTER_ID, 0x8100, 0);
                let call = engine
                    .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
                    .unwrap();
                let callback = engine
                    .begin_resident_callback(
                        KEY,
                        outer,
                        unit,
                        call.token,
                        entry,
                        RETURN,
                        RETURN_ID,
                        &[],
                    )
                    .unwrap();
                engine
                    .authorize_resident_callback(KEY, unit, callback.token)
                    .unwrap();
            } else {
                let generation = engine.compile_with_gates(3, 3).unwrap();
                stop(&mut engine, OUTER, OUTER_ID, 0x8100, 0);
                let call = engine
                    .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
                    .unwrap();
                engine
                    .begin_callback(KEY, generation, call.token, entry, RETURN, RETURN_ID, &[])
                    .unwrap();
            }
        }
    }
    #[test]
    fn synthetic_and_misidentified_gate_entries_are_rejected_without_publishing_callback_state() {
        for (entry_is_loaded, resident) in
            [(false, true), (false, false), (true, true), (true, false)]
        {
            let (mut engine, loaded) = engine();
            let entry = if entry_is_loaded { loaded } else { FAKE };
            descriptors(
                &mut engine,
                &[
                    (OUTER, OUTER_ID),
                    (entry, WindowsApi32::GetLastError.id() ^ 1),
                    (RETURN, RETURN_ID),
                ],
            );
            if resident {
                let unit = engine.compile_resident_with_gates(3, 3).unwrap().get();
                stop(&mut engine, OUTER, OUTER_ID, 0x8100, 0);
                let call = engine
                    .capture_resident_call(KEY, unit, CallingConvention32::Cdecl, 0)
                    .unwrap();
                let before = engine.arena().to_vec();
                assert_eq!(
                    engine.begin_resident_callback(
                        KEY,
                        unit,
                        unit,
                        call.token,
                        entry,
                        RETURN,
                        RETURN_ID,
                        &[]
                    ),
                    Err(HostError::Call(CallError::InvalidRequest))
                );
                assert_eq!(engine.arena(), before);
            } else {
                let generation = engine.compile_with_gates(3, 3).unwrap();
                stop(&mut engine, OUTER, OUTER_ID, 0x8100, 0);
                let call = engine
                    .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
                    .unwrap();
                let before = engine.arena().to_vec();
                assert_eq!(
                    engine.begin_callback(
                        KEY,
                        generation,
                        call.token,
                        entry,
                        RETURN,
                        RETURN_ID,
                        &[]
                    ),
                    Err(HostError::Call(CallError::InvalidRequest))
                );
                assert_eq!(engine.arena(), before);
            }
            assert!(engine.callback.is_none());
        }
    }
}
