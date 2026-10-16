use super::{AbiError, header};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleSourceRecord32 {
    pub epoch: u64,
    pub token: u64,
    pub thread: u32,
    pub ready: bool,
    pub request: Option<crate::files::FileReadRequest32>,
}
pub const MODULE_SOURCE_RECORD_SIZE: usize = 96;

pub fn encode_module_source(
    record: Option<ModuleSourceRecord32>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != MODULE_SOURCE_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    if let Some(record) = record {
        if record.epoch == 0
            || record.token == 0
            || record.thread == 0
            || record.request.is_none() && !record.ready
        {
            return Err(AbiError::Module);
        }
        if let Some(request) = record.request
            && (request.epoch == 0
                || request.request == 0
                || request.file.0 == 0
                || request.length == 0
                || request
                    .offset
                    .checked_add(u64::from(request.length))
                    .is_none())
        {
            return Err(AbiError::Module);
        }
    }
    header::write(output, *b"R3MS");
    let Some(record) = record else {
        return Ok(());
    };
    header::write_u32(output, 12, 1);
    for (offset, value) in [(16, record.epoch), (24, record.token)] {
        header::write_u32(output, offset, value as u32);
        header::write_u32(output, offset + 4, (value >> 32) as u32);
    }
    if let Some(request) = record.request {
        for (offset, value) in [
            (40, request.epoch),
            (48, request.request),
            (56, request.file.0),
            (64, request.revision),
            (72, request.asset),
            (80, request.offset),
        ] {
            header::write_u32(output, offset, value as u32);
            header::write_u32(output, offset + 4, (value >> 32) as u32);
        }
        header::write_u32(output, 88, request.length);
    }
    header::write_u32(output, 32, record.thread);
    header::write_u32(output, 36, u32::from(record.ready));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_wait_literal_is_distinct_from_initializer_and_preserves_wide_file_identity() {
        let mut bytes = [0xff; MODULE_SOURCE_RECORD_SIZE];
        let wait = ModuleSourceRecord32 {
            epoch: 0x1122334455667788,
            token: 0x0102030405060708,
            thread: 17,
            ready: true,
            request: Some(crate::files::FileReadRequest32 {
                epoch: 19,
                request: 23,
                file: crate::files::FileId(29),
                revision: 31,
                asset: 37,
                offset: 0x100000001,
                length: 41,
            }),
        };
        encode_module_source(Some(wait), &mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"R3MS");
        assert_eq!(&bytes[8..16], &[96, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(
            &bytes[16..24],
            &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(&bytes[32..40], &[17, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(
            &bytes[80..96],
            &[1, 0, 0, 0, 1, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0]
        );
        encode_module_source(None, &mut bytes).unwrap();
        assert!(bytes[12..].iter().all(|&byte| byte == 0));
    }
}
