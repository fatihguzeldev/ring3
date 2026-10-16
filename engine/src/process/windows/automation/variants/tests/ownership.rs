use super::*;
use crate::process::HostError;

#[test]
fn native_null_bstr_variant_copy_and_copyind_produce_owned_empty_strings() {
    let mut engine = initialized();
    put(&mut engine, 0x1400, Variant32::new(8, &0u32.to_le_bytes()));
    put(&mut engine, 0x1500, Variant32::EMPTY);
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]), 0);
    let pointer = value(&engine, 0x1500).pointer();
    assert_ne!(pointer, 0);
    assert_eq!(call(&mut engine, Api::SysStringByteLen, &[pointer]), 0);
    assert_eq!(read(&engine, pointer, 2), [0; 2]);
    call(&mut engine, Api::VariantClear, &[0x1500]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1600), &[0; 4])
        .unwrap();
    put(
        &mut engine,
        0x1400,
        Variant32::new(0x4008, &0x1600u32.to_le_bytes()),
    );
    assert_eq!(call(&mut engine, Api::VariantCopyInd, &[0x1500, 0x1400]), 0);
    assert_ne!(value(&engine, 0x1500).pointer(), 0);
    call(&mut engine, Api::VariantClear, &[0x1500]);
    assert_eq!(engine.automation.bytes, 0);
    assert!(engine.automation.strings.is_empty());
}

#[test]
fn captured_variant_bstr_copy_and_byref_copy_have_independent_ownership_at_the_live_capacity() {
    let mut engine = initialized();
    engine
        .configure_automation(crate::windows::automation::AutomationLimits32 {
            objects: 2,
            ..Default::default()
        })
        .unwrap();
    let pointer = text(&mut engine, 0x1100, "a\0z");
    put(
        &mut engine,
        0x1400,
        Variant32::new(8, &pointer.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]), 0);
    let first = value(&engine, 0x1500).pointer();
    assert_ne!(first, pointer);
    assert_eq!(read(&engine, first, 8), [97, 0, 0, 0, 122, 0, 0, 0]);
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]), 0);
    assert_eq!(engine.automation.strings.len(), 2);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1600), &pointer.to_le_bytes())
        .unwrap();
    put(
        &mut engine,
        0x1700,
        Variant32::new(0x4008, &0x1600u32.to_le_bytes()),
    );
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1700]), 0);
    assert_eq!(value(&engine, 0x1500), value(&engine, 0x1700));
    assert_eq!(engine.automation.strings.len(), 1);
    assert_eq!(call(&mut engine, Api::VariantCopyInd, &[0x1500, 0x1700]), 0);
    assert_ne!(value(&engine, 0x1500).pointer(), pointer);
    call(&mut engine, Api::VariantClear, &[0x1400]);
    assert_eq!(
        read(&engine, value(&engine, 0x1500).pointer(), 8),
        [97, 0, 0, 0, 122, 0, 0, 0]
    );
    call(&mut engine, Api::VariantClear, &[0x1500]);
    assert_eq!(engine.automation.bytes, 0);
}

#[test]
fn variant_array_copy_preserves_data_after_source_clear_and_locked_destination_failure_is_neutral()
{
    let mut engine = initialized();
    let array = call(&mut engine, Api::SafeArrayCreateVector, &[3, 0, 2]);
    call(&mut engine, Api::SafeArrayAccessData, &[array, 0x1300]);
    let data = u32::from_le_bytes(read(&engine, 0x1300, 4).try_into().unwrap());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(data),
            &[0x1234_5678u32.to_le_bytes(), 0x8765_4321u32.to_le_bytes()].concat(),
        )
        .unwrap();
    call(&mut engine, Api::SafeArrayUnaccessData, &[array]);
    put(
        &mut engine,
        0x1400,
        Variant32::new(0x2003, &array.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]), 0);
    let copy = value(&engine, 0x1500).pointer();
    assert_ne!(copy, array);
    call(&mut engine, Api::SafeArrayLock, &[copy]);
    let old = value(&engine, 0x1500);
    let bytes = engine.automation.bytes;
    assert_eq!(call(&mut engine, Api::VariantClear, &[0x1500]), 0x8002_000d);
    assert_eq!(value(&engine, 0x1500), old);
    assert_eq!(engine.automation.bytes, bytes);
    assert_eq!(call(&mut engine, Api::VariantCopy, &[0x1500, 0x1500]), 0);
    call(&mut engine, Api::SafeArrayUnlock, &[copy]);
    call(&mut engine, Api::VariantClear, &[0x1400]);
    call(&mut engine, Api::SafeArrayAccessData, &[copy, 0x1300]);
    let data = u32::from_le_bytes(read(&engine, 0x1300, 4).try_into().unwrap());
    assert_eq!(
        read(&engine, data, 8),
        [0x1234_5678u32.to_le_bytes(), 0x8765_4321u32.to_le_bytes()].concat()
    );
    call(&mut engine, Api::SafeArrayUnaccessData, &[copy]);
    call(&mut engine, Api::VariantClear, &[0x1500]);
    assert_eq!(engine.automation.bytes, 0);
    assert!(engine.automation.arrays.is_empty());
}

#[test]
fn variant_bad_output_and_indirect_cycles_preserve_old_owned_value_and_heap() {
    let mut engine = initialized();
    let pointer = text(&mut engine, 0x1100, "held");
    put(
        &mut engine,
        0x1400,
        Variant32::new(8, &pointer.to_le_bytes()),
    );
    let bytes = engine.automation.bytes;
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_automation_variant_operation(Api::VariantCopy, &[0xfffffff0, 0x1400]),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.automation.bytes, bytes);
    put(
        &mut engine,
        0x1600,
        Variant32::new(0x400c, &0x1600u32.to_le_bytes()),
    );
    assert_eq!(
        call(&mut engine, Api::VariantCopyInd, &[0x1400, 0x1600]),
        0x8007_0057
    );
    assert_eq!(
        value(&engine, 0x1400),
        Variant32::new(8, &pointer.to_le_bytes())
    );
    assert_eq!(engine.automation.bytes, bytes);
    call(&mut engine, Api::VariantClear, &[0x1400]);
    assert_eq!(engine.automation.bytes, 0);
}
