use crate::{
    abi::header,
    process::{ExecutionFrameStart32, ExecutionReceipt32},
};

pub const EXECUTION_FRAME_SIZE: usize = 32;
pub const EXECUTION_RECEIPT_SIZE: usize = 64;

pub fn encode_execution_frame(
    frame: ExecutionFrameStart32,
    output: &mut [u8; EXECUTION_FRAME_SIZE],
) {
    header::write(output, *b"R3EF");
    output[16..24].copy_from_slice(&frame.token.to_le_bytes());
    output[24..32].copy_from_slice(&frame.start_epoch.to_le_bytes());
}

pub fn encode_execution_receipt(
    receipt: ExecutionReceipt32,
    output: &mut [u8; EXECUTION_RECEIPT_SIZE],
) {
    header::write(output, *b"R3ER");
    output[16..24].copy_from_slice(&receipt.epoch.to_le_bytes());
    output[24..32].copy_from_slice(&receipt.owner.to_le_bytes());
    for (offset, value) in [
        (32, receipt.slot),
        (36, receipt.total_retired),
        (40, receipt.child_retired),
        (44, receipt.reason),
        (48, receipt.kind),
    ] {
        header::write_u32(output, offset, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_records_keep_literal_offsets_reserved_zeros_and_output_bounds() {
        let mut begin = [0xa5; 40];
        encode_execution_frame(
            ExecutionFrameStart32 {
                token: 0x0123456789abcdef,
                start_epoch: 0xfedcba9876543210,
            },
            (&mut begin[4..36]).try_into().unwrap(),
        );
        assert_eq!(
            &begin[4..36],
            &[
                82, 51, 69, 70, 1, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 239, 205, 171, 137, 103, 69,
                35, 1, 16, 50, 84, 118, 152, 186, 220, 254
            ]
        );
        assert_eq!(&begin[..4], &[0xa5; 4]);
        assert_eq!(&begin[36..], &[0xa5; 4]);
        let mut end = [0xa5; 72];
        encode_execution_receipt(
            ExecutionReceipt32 {
                epoch: 0x0123456789abcdef,
                owner: 0xfedcba9876543210,
                slot: 0x89abcdef,
                total_retired: 9,
                child_retired: 4,
                reason: 23,
                kind: 3,
            },
            (&mut end[4..68]).try_into().unwrap(),
        );
        assert_eq!(
            &end[4..68],
            &[
                82, 51, 69, 82, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0, 239, 205, 171, 137, 103, 69,
                35, 1, 16, 50, 84, 118, 152, 186, 220, 254, 239, 205, 171, 137, 9, 0, 0, 0, 4, 0,
                0, 0, 23, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(&end[..4], &[0xa5; 4]);
        assert_eq!(&end[68..], &[0xa5; 4]);
    }
}
