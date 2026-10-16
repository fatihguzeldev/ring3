use super::*;
#[test]
fn find_load_size_lock_and_loadstring_use_real_module_memory() {
    let mut engine = engine(false, 8);
    let resource = value(&mut engine, Api::FindResourceW, &[0, 7, 10]).unwrap();
    assert_eq!(
        value(&mut engine, Api::FindResourceW, &[0, 7, 10]),
        Ok(resource)
    );
    assert_eq!(
        value(&mut engine, Api::SizeofResource, &[0, resource]),
        Ok(4)
    );
    let data = value(&mut engine, Api::LoadResource, &[0, resource]).unwrap();
    assert_eq!(data, BASE + 0x2180);
    assert_eq!(value(&mut engine, Api::LockResource, &[data]), Ok(data));
    assert_eq!(read::<4>(&engine, data), [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(
        value(&mut engine, Api::LoadStringW, &[0, 0, BASE + 0x2300, 4]),
        Ok(3)
    );
    assert_eq!(
        read::<8>(&engine, BASE + 0x2300),
        [b'h', 0, b'e', 0, b'l', 0, 0, 0]
    );
    assert_eq!(
        value(&mut engine, Api::LoadStringW, &[0, 0, BASE + 0x2300, 0]),
        Ok(5)
    );
    assert_eq!(
        u32::from_le_bytes(read::<4>(&engine, BASE + 0x2300)),
        BASE + 0x2102
    );
    assert_eq!(
        value(&mut engine, Api::LoadStringA, &[0, 0, BASE + 0x2300, 8]),
        Ok(5)
    );
    assert_eq!(read::<6>(&engine, BASE + 0x2300), *b"hello\0");
    assert_eq!(value(&mut engine, Api::FreeResource, &[data]), Ok(0));
    assert_eq!(value(&mut engine, Api::LockResource, &[data]), Ok(data));
}
#[test]
fn bad_string_output_is_neutral_and_old_resource_identity_does_not_alias_reloaded_dll() {
    let mut engine = engine(false, 1);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(value(&mut engine, Api::LoadStringW, &[0, 0, 0xfffffff0, 0]).is_err());
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    register(&mut engine, "resource.dll", &image(0x500000, true, false));
    assert!(matches!(
        engine.load_library_module("resource.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x500000 })
    ));
    let old = value(&mut engine, Api::FindResourceW, &[0x500000, 7, 10]).unwrap();
    let old_data = value(&mut engine, Api::LoadResource, &[0x500000, old]).unwrap();
    assert_eq!(
        engine.free_library_module(0x500000),
        Ok(ModuleOperation32::Released)
    );
    assert!(matches!(
        engine.load_library_module("resource.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x500000 })
    ));
    assert_eq!(
        value(&mut engine, Api::SizeofResource, &[0x500000, old]),
        Err(ResourceError32::InvalidHandle)
    );
    assert_eq!(
        value(&mut engine, Api::LockResource, &[old_data]),
        Err(ResourceError32::InvalidHandle)
    );
    let new = value(&mut engine, Api::FindResourceW, &[0x500000, 7, 10]).unwrap();
    assert_ne!(new, old);
}
