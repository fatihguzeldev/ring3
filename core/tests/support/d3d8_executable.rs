use super::imported_executable;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

pub fn pe32(width: u32, height: u32, color: u32) -> Vec<u8> {
    let mut code = Vec::new();
    push(&mut code, 120);
    code.extend_from_slice(&[
        0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6, 0x8b, 0x06, 0x56, 0xff, 0x50, 0x10, 0x8b, 0x06,
    ]);
    for value in [0x0040_2200, 2, 0] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x56, 0xff, 0x50, 0x14, 0x8b, 0x06]);
    for value in [0x0040_2180, 0x0040_2100, 0x20, 1, 1, 0] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[
        0x56, 0xff, 0x50, 60, 0x8b, 0x35, 0x80, 0x21, 0x40, 0, 0x8b, 0x06,
    ]);
    for value in [0, 0, color, 1, 0, 0] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x56, 0xff, 0x90, 144, 0, 0, 0, 0x8b, 0x06]);
    for _ in 0..4 {
        push(&mut code, 0);
    }
    code.extend_from_slice(&[0x56, 0xff, 0x50, 60, 0xcc]);
    let mut bytes = imported_executable::pe32(&code, "d3d8.dll", &["Direct3DCreate8"]);
    let params = [width, height, 22, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0];
    for (index, value) in params.iter().enumerate() {
        let offset = 1280 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
