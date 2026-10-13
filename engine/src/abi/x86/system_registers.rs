use crate::{
    abi::{
        AbiError,
        header::{self, read_u32, write_u32},
    },
    cpu::x86::{DescriptorTableRegister32, UserSystemRegisterBank32},
};

pub const SYSTEM_REGISTER_BANK_SIZE: usize = 36;

pub fn encode_system_register_bank(
    bank: &UserSystemRegisterBank32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != SYSTEM_REGISTER_BANK_SIZE {
        return Err(AbiError::Length);
    }
    if !bank.is_valid() {
        return Err(AbiError::Reserved);
    }
    let mut staged = [0; SYSTEM_REGISTER_BANK_SIZE];
    header::write(&mut staged, *b"R3SY");
    let mut present = 0;
    for (bit, value, base, limit) in [(0, bank.gdtr, 16, 24), (1, bank.idtr, 20, 26)] {
        if let Some(value) = value {
            present |= 1 << bit;
            write_u32(&mut staged, base, value.base);
            staged[limit..limit + 2].copy_from_slice(&value.limit.to_le_bytes());
        }
    }
    for (bit, value, offset) in [
        (2, bank.ldtr, 28),
        (3, bank.tr, 30),
        (4, bank.cr0_low16, 32),
    ] {
        if let Some(value) = value {
            present |= 1 << bit;
            staged[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    write_u32(&mut staged, 12, present);
    output.copy_from_slice(&staged);
    Ok(())
}

pub fn decode_system_register_bank(input: &[u8]) -> Result<UserSystemRegisterBank32, AbiError> {
    let (_, present) =
        header::validate_versions_with_metadata(input, *b"R3SY", SYSTEM_REGISTER_BANK_SIZE, &[1])?;
    if present == 0 || present & !31 != 0 || input[34..36] != [0, 0] {
        return Err(AbiError::Reserved);
    }
    let word = |offset| u16::from_le_bytes(input[offset..offset + 2].try_into().unwrap());
    let bank = UserSystemRegisterBank32 {
        gdtr: (present & 1 != 0).then(|| DescriptorTableRegister32 {
            base: read_u32(input, 16),
            limit: word(24),
        }),
        idtr: (present & 2 != 0).then(|| DescriptorTableRegister32 {
            base: read_u32(input, 20),
            limit: word(26),
        }),
        ldtr: (present & 4 != 0).then(|| word(28)),
        tr: (present & 8 != 0).then(|| word(30)),
        cr0_low16: (present & 16 != 0).then(|| word(32)),
    };
    let mut canonical = [0; SYSTEM_REGISTER_BANK_SIZE];
    encode_system_register_bank(&bank, &mut canonical)?;
    if canonical != input {
        return Err(AbiError::Reserved);
    }
    Ok(bank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_zero_and_absence_have_distinct_strict_canonical_records() {
        for bank in [
            UserSystemRegisterBank32 {
                ldtr: Some(0),
                ..Default::default()
            },
            UserSystemRegisterBank32 {
                gdtr: Some(DescriptorTableRegister32 {
                    base: 0xfedc_ba98,
                    limit: 0x3456,
                }),
                cr0_low16: Some(0x33),
                ..Default::default()
            },
        ] {
            let mut bytes = [0xa5; SYSTEM_REGISTER_BANK_SIZE];
            encode_system_register_bank(&bank, &mut bytes).unwrap();
            assert_eq!(decode_system_register_bank(&bytes), Ok(bank));
            for at in [0, 4, 6, 8, 15, 34] {
                let mut bad = bytes;
                bad[at] ^= 0x80;
                assert!(decode_system_register_bank(&bad).is_err());
            }
        }
        let mut output = [0xa5; SYSTEM_REGISTER_BANK_SIZE];
        assert!(
            encode_system_register_bank(&UserSystemRegisterBank32::default(), &mut output).is_err()
        );
        assert_eq!(output, [0xa5; SYSTEM_REGISTER_BANK_SIZE]);
        let bank = UserSystemRegisterBank32 {
            ldtr: Some(0),
            ..Default::default()
        };
        encode_system_register_bank(&bank, &mut output).unwrap();
        output[16] = 1;
        assert_eq!(
            decode_system_register_bank(&output),
            Err(AbiError::Reserved)
        );
    }
}
