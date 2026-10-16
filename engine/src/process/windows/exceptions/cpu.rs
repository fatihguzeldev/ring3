use crate::{cpu::ExitReason, process::CpuEvent32, windows::exceptions::ExceptionRecord32};

pub(super) fn windows_cpu_exception_record(event: &CpuEvent32) -> Option<ExceptionRecord32> {
    let mut record = ExceptionRecord32::from_cpu_event(event)?;
    if matches!(
        event.exit.reason,
        ExitReason::GeneralProtection { error_code: 0 }
    ) && event.is_privileged_instruction()
    {
        // windows' ia-32 privileged-instruction status is distinct from selector/access faults.
        record.code = 0xc0000096;
        record.count = 0;
        record.information.fill(0);
    }
    if event.guard_page.is_some() {
        record.code = 0x80000001;
    }
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::{
            arena::{EXIT_OFFSET, TRANSFER_OFFSET},
            x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_exit_v7, encode_state_v3},
        },
        cpu::{
            ExecutionExit,
            x86::{EventOrigin32, ExceptionCause32, SelectorErrorCode32, State32},
        },
        memory::GuestAddress,
        process::{
            EngineInstance, calls::call::PendingOwner, cpu::events::detection::DetectedEvent32,
        },
    };

    const KEY: u64 = 0x50524956;

    fn sealed_gp(
        bytes: &[u8],
        resident: bool,
        error_code: u32,
        replace_before_seal: bool,
    ) -> (EngineInstance, CpuEvent32) {
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x7000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(0x1000, bytes.len() as u32).unwrap();
        let mut state = State32 {
            eip: 0x1000,
            ..State32::default()
        };
        state.registers[4] = 0x7800;
        encode_state_v3(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        let owner = if resident {
            let id = engine.compile_stopped_resident_entry(KEY).unwrap().get();
            engine.guard_material_resident(KEY, id).unwrap();
            PendingOwner::Resident(id)
        } else {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(0x1100), &[0xeb, 0xfe])
                .unwrap();
            let descriptor = [0x1100u32.to_le_bytes(), 2u32.to_le_bytes()].concat();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
                .copy_from_slice(&descriptor);
            let bootstrap = engine.compile(1).unwrap();
            let generation = engine.compile_stopped_entry(KEY, bootstrap).unwrap();
            engine.guard_material(KEY, generation).unwrap();
            PendingOwner::Replacement(generation)
        };
        let epoch = engine.begin_exception_detection(owner).unwrap();
        if replace_before_seal {
            engine
                .memory
                .as_mut()
                .unwrap()
                .cpu_write(GuestAddress(0x1000), &[0x90])
                .unwrap();
        }
        let arena = engine.arena_mut().unwrap();
        encode_state_v3(&state, &mut arena[..STATE_SIZE]).unwrap();
        encode_exit_v7(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::GeneralProtection { error_code },
            },
            &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        engine
            .finish_exception_detection(
                epoch,
                DetectedEvent32::Exception(EventOrigin32::Exception(
                    ExceptionCause32::GeneralProtection(
                        SelectorErrorCode32::from_bits(error_code).unwrap(),
                    ),
                )),
                None,
            )
            .unwrap();
        let event = engine
            .consume_exception_detection(epoch)
            .unwrap()
            .into_event();
        (engine, event)
    }

    #[test]
    fn privileged_opcodes_translate_gp_zero_to_windows_privileged_instruction() {
        // original-windows ia-32 oracle: wine dlls/ntdll/tests/exception.c, exception cases 3..13.
        for bytes in [
            &[0xf4][..],
            &[0xfa],
            &[0xfb],
            &[0xe4, 0x01],
            &[0xe6, 0x01],
            &[0x6e],
            &[0x0f, 0x20, 0xc0],
            &[0x0f, 0x21, 0xc0],
        ] {
            for resident in [false, true] {
                let (_, event) = sealed_gp(bytes, resident, 0, false);
                assert!(event.is_privileged_instruction(), "{bytes:02x?}");
                let record = windows_cpu_exception_record(&event).unwrap();
                assert_eq!(
                    (record.code, record.address, record.count),
                    (0xc0000096, 0x1000, 0)
                );
                assert_eq!(record.information, [0; 15]);
            }
        }
    }

    #[test]
    fn selector_interrupt_and_overlong_prefix_gp_keep_access_fault_classification() {
        for (bytes, error_code) in [
            (&[0x8e, 0xd8][..], 0x20),
            (&[0xcd, 0x13][..], 0x9a),
            (&[0x66; 15][..], 0),
        ] {
            let (_, event) = sealed_gp(bytes, true, error_code, false);
            assert!(!event.is_privileged_instruction(), "{bytes:02x?}");
            let record = windows_cpu_exception_record(&event).unwrap();
            assert_eq!((record.code, record.count), (0xc0000005, 2));
            assert_eq!(&record.information[..2], &[0, u32::MAX]);
        }
    }

    #[test]
    fn sealed_privileged_class_survives_source_writes_before_and_after_publication() {
        for resident in [false, true] {
            for before in [false, true] {
                let (mut engine, event) = sealed_gp(&[0xf4], resident, 0, before);
                engine
                    .memory
                    .as_mut()
                    .unwrap()
                    .cpu_write(GuestAddress(0x1000), &[0x90])
                    .unwrap();
                assert!(event.matches_context(engine.arena()));
                assert_eq!(
                    windows_cpu_exception_record(&event).unwrap().code,
                    0xc0000096
                );
            }
            let (mut engine, event) = sealed_gp(&[0x8e, 0xd8], resident, 0, false);
            engine
                .memory
                .as_mut()
                .unwrap()
                .cpu_write(GuestAddress(0x1000), &[0xf4])
                .unwrap();
            assert!(event.matches_context(engine.arena()));
            assert_eq!(
                windows_cpu_exception_record(&event).unwrap().code,
                0xc0000005
            );
        }
    }
}
