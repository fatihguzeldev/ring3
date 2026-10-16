use super::*;
#[test]
fn ansi_set_unicode_query_and_getvalue_termination_use_bytes_not_high_zero_bytes() {
    let mut engine = engine();
    let handle = create(&mut engine);
    put(&mut engine, 0x2200, b"Name\0");
    put(&mut engine, 0x2300, b"A");
    assert_eq!(
        engine.registry_call(Api::RegSetValueExA, &[handle, 0x2200, 0, 1, 0x2300, 1], 200),
        Ok(0)
    );
    put(
        &mut engine,
        0x2400,
        &[b'N', 0, b'a', 0, b'm', 0, b'e', 0, 0, 0],
    );
    put(&mut engine, 0x2500, &4u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegGetValueW,
            &[handle, 0, 0x2400, 2, 0x2504, 0x2600, 0x2500],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 4);
    assert_eq!(word(&engine, 0x2504), 1);
    assert_eq!(
        memory::read::<4>(engine.memory.as_ref().unwrap(), 0x2600).unwrap(),
        [b'A', 0, 0, 0]
    );
    put(&mut engine, 0x2500, &1u32.to_le_bytes());
    put(&mut engine, 0x2600, &[0xcc; 4]);
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExA,
            &[handle, 0x2200, 0, 0x2504, 0x2600, 0x2500],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 1);
    assert_eq!(
        memory::read::<4>(engine.memory.as_ref().unwrap(), 0x2600).unwrap(),
        [b'A', 0xcc, 0xcc, 0xcc]
    );
}
#[test]
fn more_data_preserves_data_and_zeroonfailure_zeros_only_original_capacity() {
    let mut engine = engine();
    let handle = create(&mut engine);
    put(&mut engine, 0x2300, &[1, 2, 3, 4]);
    assert_eq!(
        engine.registry_call(Api::RegSetValueExW, &[handle, 0, 0, 3, 0x2300, 4], 200),
        Ok(0)
    );
    put(&mut engine, 0x2500, &2u32.to_le_bytes());
    put(&mut engine, 0x2600, &[0xcc; 4]);
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExW,
            &[handle, 0, 0, 0x2504, 0x2600, 0x2500],
            200
        ),
        Ok(234)
    );
    assert_eq!(word(&engine, 0x2500), 4);
    assert_eq!(word(&engine, 0x2600), 0xcccccccc);
    put(&mut engine, 0x2500, &2u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegGetValueW,
            &[handle, 0, 0, 0x20000008, 0, 0x2600, 0x2500],
            200
        ),
        Ok(234)
    );
    assert_eq!(
        memory::read::<4>(engine.memory.as_ref().unwrap(), 0x2600).unwrap(),
        [0, 0, 0xcc, 0xcc]
    );
}
