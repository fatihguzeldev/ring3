use super::executable;

pub fn pe32(code: &[u8], module: &str, names: &[&str]) -> Vec<u8> {
    assert!(module.len() < 32 && names.len() <= 4);
    let mut bytes = executable::pe32(code);
    bytes[1024..].fill(0);
    let mut put = |offset: usize, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    put(0x100, 0x2000);
    put(0x104, 40);
    put(1024, 0x2040);
    put(1036, 0x2080);
    put(1040, 0x2060);
    for (index, _) in names.iter().enumerate() {
        let rva = 0x20c0 + u32::try_from(index).unwrap() * 48;
        put(1088 + index * 4, rva);
        put(1120 + index * 4, rva);
    }
    bytes[1152..1152 + module.len()].copy_from_slice(module.as_bytes());
    for (index, name) in names.iter().enumerate() {
        assert!(name.len() < 46);
        let offset = 1218 + index * 48;
        bytes[offset..offset + name.len()].copy_from_slice(name.as_bytes());
    }
    bytes
}
