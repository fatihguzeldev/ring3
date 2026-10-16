use super::DeviceName32;
fn same(units: &[u16], ascii: &[u8]) -> bool {
    units.len() == ascii.len()
        && units.iter().zip(ascii).all(|(&unit, &byte)| {
            unit == u16::from(byte) || (unit <= 127 && (unit as u8).to_ascii_uppercase() == byte)
        })
}
pub(crate) fn device_name(path: &[u16]) -> Option<DeviceName32> {
    if path.starts_with(&[92, 92, 63, 92]) {
        return None;
    }
    let explicit = path.starts_with(&[92, 92, 46, 92]);
    if !explicit && path.starts_with(&[92, 92]) {
        return None;
    }
    let path = if explicit { &path[4..] } else { path };
    let name = if explicit {
        path
    } else {
        path.rsplit(|unit| matches!(unit, 92 | 47)).next()?
    };
    let name = if !explicit && name.get(1) == Some(&58) {
        &name[2..]
    } else {
        name
    };
    let mut end = name.len();
    while end != 0 && matches!(name[end - 1], 32 | 46) {
        end -= 1;
    }
    let name = &name[..end];
    let base = if explicit {
        name
    } else {
        name.split(|unit| matches!(unit, 46 | 58)).next()?
    };
    if same(base, b"NUL") {
        Some(DeviceName32::Null)
    } else if same(base, b"CON") {
        Some(DeviceName32::Console)
    } else if same(name, b"CONIN$") {
        Some(DeviceName32::ConsoleInput)
    } else if same(name, b"CONOUT$") {
        Some(DeviceName32::ConsoleOutput)
    } else {
        None
    }
}
