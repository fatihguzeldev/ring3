use super::{InterfaceId, ServerThreadingModel32};
use crate::windows::{ole32::HResult32, registry::Registry32};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InProcessServer32 {
    pub path: String,
    pub threading: ServerThreadingModel32,
}

pub fn class_registry_path(class: InterfaceId) -> String {
    let b = class.0;
    format!(
        "CLSID\\{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}\\InprocServer32",
        u32::from_le_bytes(b[..4].try_into().unwrap()),
        u16::from_le_bytes(b[4..6].try_into().unwrap()),
        u16::from_le_bytes(b[6..8].try_into().unwrap()),
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

pub(crate) fn registered_server(
    registry: &Registry32,
    class: InterfaceId,
    expand: impl FnOnce(&str) -> String,
) -> Result<Option<InProcessServer32>, HResult32> {
    let path: Vec<u16> = class_registry_path(class).encode_utf16().collect();
    let value = match registry.query_subkey_value(0x8000_0000, &path, 0, &[]) {
        Ok(value) => value,
        Err(crate::windows::registry::RegistryError32::FileNotFound) => return Ok(None),
        Err(_) => return Err(HResult32::CLASS_NOT_REGISTERED),
    };
    if !matches!(value.kind, 1 | 2) {
        return Err(HResult32::CLASS_NOT_REGISTERED);
    }
    let text = registry_string(value.bytes)?;
    let server_path = if value.kind == 2 { expand(&text) } else { text };
    if server_path.is_empty()
        || server_path.contains('\0')
        || server_path.encode_utf16().count() > 32767
    {
        return Err(HResult32::CLASS_NOT_REGISTERED);
    }
    let name: Vec<u16> = "ThreadingModel".encode_utf16().collect();
    let threading = match registry.query_subkey_value(0x8000_0000, &path, 0, &name) {
        Ok(value) if value.kind == 1 => {
            match registry_string(value.bytes)?.to_ascii_lowercase().as_str() {
                "apartment" => ServerThreadingModel32::Apartment,
                "free" => ServerThreadingModel32::Free,
                "both" => ServerThreadingModel32::Both,
                "neutral" => ServerThreadingModel32::Neutral,
                "" => ServerThreadingModel32::MainSta,
                _ => return Err(HResult32::CLASS_NOT_REGISTERED),
            }
        }
        Ok(_) => return Err(HResult32::CLASS_NOT_REGISTERED),
        Err(crate::windows::registry::RegistryError32::FileNotFound) => {
            ServerThreadingModel32::MainSta
        }
        Err(_) => return Err(HResult32::CLASS_NOT_REGISTERED),
    };
    Ok(Some(InProcessServer32 {
        path: server_path,
        threading,
    }))
}

fn registry_string(bytes: &[u8]) -> Result<String, HResult32> {
    if !bytes.len().is_multiple_of(2) || bytes.len() > 65536 {
        return Err(HResult32::CLASS_NOT_REGISTERED);
    }
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if units.last() == Some(&0) {
        units.pop();
    }
    if units.contains(&0) {
        return Err(HResult32::CLASS_NOT_REGISTERED);
    }
    String::from_utf16(&units).map_err(|_| HResult32::CLASS_NOT_REGISTERED)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guid_registry_spelling_uses_windows_mixed_endian_fields() {
        assert_eq!(
            class_registry_path(InterfaceId([
                0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 1, 2, 3, 4, 5, 6, 7, 8
            ])),
            "CLSID\\{12345678-9abc-def0-0102-030405060708}\\InprocServer32"
        );
        assert_eq!(registry_string(&[65, 0, 0, 0]), Ok("A".into()));
        assert!(registry_string(&[65, 0, 0, 0, 66, 0]).is_err());
        assert!(registry_string(&[0, 0xd8]).is_err());
    }
}
