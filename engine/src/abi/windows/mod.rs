use super::{AbiError, header};

pub const WINDOWS_CALLBACK_RECORD_SIZE: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsCallbackRecord32 {
    pub provider: u32,
    pub procedure: u32,
    pub count: u32,
    pub continuation: u64,
    pub arguments: [u32; 16],
}

pub fn encode_windows_callback(
    record: Option<WindowsCallbackRecord32>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != WINDOWS_CALLBACK_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    if let Some(record) = record {
        validate(record)?;
    }
    header::write(output, *b"R3WC");
    if let Some(record) = record {
        header::write_u32(output, 12, record.provider);
        header::write_u32(output, 16, record.procedure);
        header::write_u32(output, 20, record.count);
        output[24..32].copy_from_slice(&record.continuation.to_le_bytes());
        for (index, value) in record.arguments.into_iter().enumerate() {
            header::write_u32(output, 32 + index * 4, value);
        }
    }
    Ok(())
}

pub fn decode_windows_callback(input: &[u8]) -> Result<Option<WindowsCallbackRecord32>, AbiError> {
    let (_, provider) = header::validate_versions_with_metadata(
        input,
        *b"R3WC",
        WINDOWS_CALLBACK_RECORD_SIZE,
        &[1],
    )?;
    if provider == 0 {
        return if input[16..].iter().any(|byte| *byte != 0) {
            Err(AbiError::Reserved)
        } else {
            Ok(None)
        };
    }
    let record = WindowsCallbackRecord32 {
        provider,
        procedure: header::read_u32(input, 16),
        count: header::read_u32(input, 20),
        continuation: u64::from_le_bytes(input[24..32].try_into().unwrap()),
        arguments: std::array::from_fn(|index| header::read_u32(input, 32 + index * 4)),
    };
    validate(record)?;
    Ok(Some(record))
}

fn validate(record: WindowsCallbackRecord32) -> Result<(), AbiError> {
    if !matches!(record.provider, 1..=6)
        || record.procedure == 0
        || record.continuation == 0
        || record.count > 16
    {
        return Err(AbiError::CallbackFrame);
    }
    if record.arguments[record.count as usize..]
        .iter()
        .any(|word| *word != 0)
    {
        return Err(AbiError::Reserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
