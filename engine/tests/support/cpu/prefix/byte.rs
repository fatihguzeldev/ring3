use super::support_prefix_admission as prefix_admission;

use ring3_engine::cpu::x86::decode::DecodeError;

fn newly_admitted(bytes: &[u8]) -> bool {
    match bytes {
        [
            0xf0,
            0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30,
            modrm,
            ..,
        ] => modrm & 0xc0 != 0xc0,
        [0xf0, 0x80, modrm, ..] => modrm & 0xc0 != 0xc0 && (modrm >> 3) & 7 != 7,
        [0xf0, 0xfe, modrm, ..] => modrm & 0xc0 != 0xc0 && (modrm >> 3) & 7 <= 1,
        [0xf0, 0xf6, modrm, ..] => modrm & 0xc0 != 0xc0 && matches!((modrm >> 3) & 7, 2 | 3),
        [0xf0, 0x66, 0x87, modrm, ..] => modrm & 0xc0 != 0xc0,
        _ => false,
    }
}

// the historical row first proves its newly admitted behavior. a same-length malformed
// fixture then retains the original precise-failure and prior-owner assertions.
pub fn closed_fixture(bytes: &[u8], expected: DecodeError) -> (Vec<u8>, DecodeError) {
    let prefixed = prefix_admission::admit_if_new_prefix(bytes);
    if prefixed || newly_admitted(bytes) {
        if !prefixed {
            prefix_admission::admitted(bytes);
        }
        prefix_admission::malformed_fixture(bytes.len())
    } else {
        (bytes.to_vec(), expected)
    }
}
