use super::*;
use crate::windows::{nls::NlsProfile32, text::CodePage32};

#[test]
fn japanese_ansi_registry_names_and_string_lengths_count_encoded_bytes_atomically() {
    let mut engine = engine();
    engine
        .configure_nls(NlsProfile32 {
            locale: 0x411,
            ansi: CodePage32::from_id(932).unwrap(),
            oem: CodePage32::from_id(932).unwrap(),
            ..Default::default()
        })
        .unwrap();
    let handle = create(&mut engine);
    put(&mut engine, 0x2200, &[0x82, 0xa0, 0]);
    put(&mut engine, 0x2300, &[0x93, 0xfa, 0x96, 0x7b, 0]);
    assert_eq!(
        engine.registry_call(Api::RegSetValueExA, &[handle, 0x2200, 0, 1, 0x2300, 5], 200),
        Ok(0)
    );
    put(&mut engine, 0x2500, &0u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExA,
            &[handle, 0x2200, 0, 0, 0, 0x2500],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 5);
    put(&mut engine, 0x2500, &4u32.to_le_bytes());
    put(&mut engine, 0x2600, &[0xcc; 8]);
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExA,
            &[handle, 0x2200, 0, 0, 0x2600, 0x2500],
            200
        ),
        Ok(234)
    );
    assert_eq!(word(&engine, 0x2500), 5);
    assert_eq!(word(&engine, 0x2600), 0xcccccccc);
    put(&mut engine, 0x2500, &8u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExA,
            &[handle, 0x2200, 0, 0, 0x2600, 0x2500],
            200
        ),
        Ok(0)
    );
    assert_eq!(
        memory::read::<5>(engine.memory().unwrap(), 0x2600).unwrap(),
        [0x93, 0xfa, 0x96, 0x7b, 0]
    );
    put(&mut engine, 0x2500, &8u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(Api::RegQueryValueExW, &[handle, 0, 0, 0, 0, 0x2500], 200),
        Ok(2)
    );
    put(&mut engine, 0x2400, &[0x42, 0x30, 0, 0]);
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExW,
            &[handle, 0x2400, 0, 0, 0x2600, 0x2500],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 6);
    assert_eq!(
        memory::read::<6>(engine.memory().unwrap(), 0x2600).unwrap(),
        [0xe5, 0x65, 0x2c, 0x67, 0, 0]
    );
    put(&mut engine, 0x2500, &16u32.to_le_bytes());
    put(&mut engine, 0x2504, &16u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegEnumValueA,
            &[handle, 0, 0x2700, 0x2500, 0, 0, 0x2800, 0x2504],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 2);
    assert_eq!(word(&engine, 0x2504), 5);
    assert_eq!(
        memory::read::<3>(engine.memory().unwrap(), 0x2700).unwrap(),
        [0x82, 0xa0, 0]
    );
    assert_eq!(
        memory::read::<5>(engine.memory().unwrap(), 0x2800).unwrap(),
        [0x93, 0xfa, 0x96, 0x7b, 0]
    );
    assert_eq!(
        engine.registry_call(
            Api::RegQueryInfoKeyA,
            &[handle, 0, 0, 0, 0, 0, 0, 0, 0x2500, 0x2504, 0, 0],
            200
        ),
        Ok(0)
    );
    assert_eq!(word(&engine, 0x2500), 2);
    assert_eq!(word(&engine, 0x2504), 5);
    put(&mut engine, 0x2500, &16u32.to_le_bytes());
    put(&mut engine, 0x2504, &16u32.to_le_bytes());
    assert_eq!(
        engine.registry_call(
            Api::RegEnumValueA,
            &[handle, 0, 0x2700, 0x2500, 0, 0, 0xfffffff0, 0x2504],
            200
        ),
        Err(RegistryError32::NoAccess)
    );
    assert_eq!(word(&engine, 0x2500), 16);
    assert_eq!(word(&engine, 0x2504), 16);
}
