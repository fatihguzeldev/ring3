use super::{AbiError, header};

pub const MODULE_RECORD_SIZE: usize = 72;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModuleRecord32 {
    pub outcome: u32,
    pub value: u32,
    pub token: u64,
    pub module: u64,
    pub entry: u32,
    pub return_pc: u32,
    pub return_id: u32,
    pub return_esp: u32,
    pub reason: u32,
    pub kind: u32,
}

pub fn encode_module(record: &ModuleRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != MODULE_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    validate(record)?;
    header::write(output, *b"R3MO");
    for (index, value) in [
        record.outcome,
        record.value,
        record.token as u32,
        (record.token >> 32) as u32,
        record.module as u32,
        (record.module >> 32) as u32,
        record.entry,
        record.return_pc,
        record.return_id,
        record.return_esp,
        record.reason,
        record.kind,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}

pub fn decode_module(input: &[u8]) -> Result<ModuleRecord32, AbiError> {
    header::validate(input, *b"R3MO", MODULE_RECORD_SIZE)?;
    if header::read_u32(input, 64) != 0 || header::read_u32(input, 68) != 0 {
        return Err(AbiError::Reserved);
    }
    let record = ModuleRecord32 {
        outcome: header::read_u32(input, 16),
        value: header::read_u32(input, 20),
        token: u64::from(header::read_u32(input, 24))
            | (u64::from(header::read_u32(input, 28)) << 32),
        module: u64::from(header::read_u32(input, 32))
            | (u64::from(header::read_u32(input, 36)) << 32),
        entry: header::read_u32(input, 40),
        return_pc: header::read_u32(input, 44),
        return_id: header::read_u32(input, 48),
        return_esp: header::read_u32(input, 52),
        reason: header::read_u32(input, 56),
        kind: header::read_u32(input, 60),
    };
    validate(&record)?;
    Ok(record)
}

fn validate(record: &ModuleRecord32) -> Result<(), AbiError> {
    if record.outcome > 5 {
        return Err(AbiError::Module);
    }
    if record.outcome == 1 {
        if record.value != 0
            || record.token == 0
            || record.module == 0
            || record.return_id == 0
            || record.entry == record.return_pc
            || record.reason > 1
            || !(1..=2).contains(&record.kind)
        {
            return Err(AbiError::Module);
        }
    } else {
        let expected = ModuleRecord32 {
            outcome: record.outcome,
            value: record.value,
            ..Default::default()
        };
        if *record != expected || (!matches!(record.outcome, 2 | 5) && record.value != 0) {
            return Err(AbiError::Module);
        }
    }
    Ok(())
}
