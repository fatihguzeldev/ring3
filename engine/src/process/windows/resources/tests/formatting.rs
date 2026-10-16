use super::*;
use crate::process::windows::resources::types::ResourceCallOutcome32 as Outcome;

fn memory_only() -> EngineInstance {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine
}
fn text(engine: &mut EngineInstance, address: u32, value: &str) {
    let mut bytes = value
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    bytes.extend_from_slice(&[0, 0]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &bytes)
        .unwrap();
}
fn units(engine: &EngineInstance, address: u32, count: usize) -> Vec<u16> {
    let mut bytes = vec![0; count * 2];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect()
}
#[test]
fn native_from_string_and_system_messages_work_without_a_resource_runtime() {
    let mut engine = memory_only();
    text(&mut engine, 0x10000, "A%nB%%[%1!08x!]%0tail");
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0x600, 0x10000, 0, 0, 0x11000, 100, 0]
        ),
        Ok(15)
    );
    assert_eq!(
        units(&engine, 0x11000, 16),
        "A\r\nB%%[%1!08x!]\0".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0x1200, 0xfffffff0, 5, 0, 0x11000, 100, 0]
        ),
        Ok(19)
    );
    assert_eq!(
        units(&engine, 0x11000, 20),
        "Access is denied.\r\n\0".encode_utf16().collect::<Vec<_>>()
    );
    assert!(matches!(
        engine.resource_call(Api::FormatMessageW, &[0x1200, 0, 5, 0x407, 0x11000, 100, 0]),
        Err(ResourceError32::Win32(1815))
    ));
    assert!(matches!(
        engine.resource_call(
            Api::FormatMessageW,
            &[0x1200, 0, 0xfefefefe, 0, 0x11000, 100, 0]
        ),
        Err(ResourceError32::Win32(317))
    ));
}
#[test]
fn native_small_output_keeps_a_terminated_prefix_and_does_not_write_beyond_capacity() {
    let mut engine = memory_only();
    text(&mut engine, 0x10000, "1234");
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x11000),
            &[0x34, 0x12, 0x45, 0x23, 0x56, 0x34, 0x67, 0x45],
        )
        .unwrap();
    assert!(matches!(
        engine.resource_call(Api::FormatMessageW, &[0x400, 0x10000, 0, 0, 0x11000, 3, 0]),
        Ok(Outcome::Return {
            value: 0,
            last_error: Some(122)
        })
    ));
    assert_eq!(units(&engine, 0x11000, 4), [49, 50, 0, 0x4567]);
}
#[test]
fn native_variable_arguments_and_array_stars_use_guest_words_and_bounded_strings() {
    let mut engine = memory_only();
    text(&mut engine, 0x10000, "%1!*.*s!/%4/%5!*s!");
    text(&mut engine, 0x10200, "Ned");
    text(&mut engine, 0x10300, "Sue");
    let words = [4u32, 2, 0x10200, 0x10300, 6, 0x10200];
    let bytes = words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10400), &bytes)
        .unwrap();
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0x2400, 0x10000, 0, 0, 0x11000, 100, 0x10400]
        ),
        Ok(15)
    );
    assert_eq!(
        units(&engine, 0x11000, 16),
        "  Ne/Sue/   Ned\0".encode_utf16().collect::<Vec<_>>()
    );
    text(&mut engine, 0x10000, "%1!*.*s!/%3/%4!*s!");
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10500), &0x10400u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0x400, 0x10000, 0, 0, 0x11000, 100, 0x10500]
        ),
        Ok(15)
    );
    assert_eq!(
        units(&engine, 0x11000, 16),
        "  Ne/Sue/   Ned\0".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn message_allocations_are_local_free_owned_and_bad_pointer_publication_is_neutral() {
    let mut engine = engine(false, 4);
    engine.map(0x10000, 2, 3).unwrap();
    text(&mut engine, 0x10000, "hello %1!u!");
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10400), &7u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0x2500, 0x10000, 0, 0, 0x11000, 100, 0x10400]
        ),
        Ok(7)
    );
    let pointer = u32::from_le_bytes(read::<4>(&engine, 0x11000));
    assert_ne!(pointer, 0);
    assert_eq!(
        units(&engine, pointer, 8),
        "hello 7\0".encode_utf16().collect::<Vec<_>>()
    );
    engine.check_windows_runtime_bytes(pointer, 200).unwrap();
    engine.free_windows_runtime_bytes(pointer);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(
        engine
            .resource_call(
                Api::FormatMessageW,
                &[0x2500, 0x10000, 0, 0, 0xfffffff0, 100, 0x10400]
            )
            .is_err()
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
}
#[test]
fn module_message_tables_use_the_canonical_pe_reader_and_exact_explicit_language() {
    let mut bytes = image(BASE, false, false);
    bytes[0x418..0x41c].copy_from_slice(&11u32.to_le_bytes());
    let table = [
        1, 0, 0, 0, 7, 0, 0, 0, 7, 0, 0, 0, 16, 0, 0, 0, 10, 0, 1, 0, b'h', 0, b'i', 0, 0, 0,
    ];
    bytes[0x4c4..0x4c8].copy_from_slice(&(table.len() as u32).to_le_bytes());
    bytes[0x580..0x580 + table.len()].copy_from_slice(&table);
    let mut engine = engine_with_image(bytes, 4, Default::default());
    engine.map(0x10000, 2, 3).unwrap();
    assert_eq!(
        value(
            &mut engine,
            Api::FormatMessageW,
            &[0xa00, 0, 7, 0x409, 0x11000, 100, 0]
        ),
        Ok(2)
    );
    assert_eq!(units(&engine, 0x11000, 3), [104, 105, 0]);
    assert!(matches!(
        engine.resource_call(Api::FormatMessageW, &[0xa00, 0, 7, 0x407, 0x11000, 100, 0]),
        Err(ResourceError32::Win32(1815))
    ));
}
