use super::*;

#[test]
fn readback_receipt_pins_request_resource_version_and_canary_tail() {
    let receipt = ReadbackReceipt {
        id: ReadbackId {
            epoch: 0x1122_3344_5566_7788,
            sequence: 7,
        },
        source: source(),
        status: ReadbackStatus::Complete,
        bytes: &[4, 5, 6],
    };
    let mut output = [0xa5; 71];
    assert_eq!(encode_readback_receipt(receipt, &mut output), Ok(67));
    assert_eq!(
        &output[..16],
        &[0x52, 0x33, 0x52, 0x38, 2, 0, 8, 0, 67, 0, 0, 0, 3, 0, 0, 0]
    );
    assert_eq!(
        &output[32..64],
        &[
            77, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0,
            0, 0, 0
        ]
    );
    assert_eq!(&output[64..], &[4, 5, 6, 0xa5, 0xa5, 0xa5, 0xa5]);
    assert_eq!(readback_receipt(&output[..67]), Ok(receipt));
    output[60] = 4;
    assert_eq!(
        readback_receipt(&output[..67]),
        Err(CodecError::InvalidCommand)
    );
}

#[test]
fn invalid_failure_payload_and_identity_leave_output_untouched() {
    let mut output = [0xa5; 80];
    let receipt = ReadbackReceipt {
        id: ReadbackId {
            epoch: 1,
            sequence: 2,
        },
        source: source(),
        status: ReadbackStatus::DeviceLost,
        bytes: &[1],
    };
    assert_eq!(
        encode_readback_receipt(receipt, &mut output),
        Err(CodecError::InvalidCommand)
    );
    assert_eq!(output, [0xa5; 80]);
    let invalid = ReadbackReceipt {
        id: ReadbackId {
            epoch: 0,
            sequence: 2,
        },
        bytes: &[],
        ..receipt
    };
    assert_eq!(
        encode_readback_receipt(invalid, &mut output),
        Err(CodecError::Identity)
    );
    assert_eq!(output, [0xa5; 80]);
}

#[test]
fn staged_header_pins_large_length_without_accepting_overflow_or_failure_bytes() {
    let receipt = ReadbackReceipt {
        id: ReadbackId { epoch: 1, sequence: 2 }, source: source(),
        status: ReadbackStatus::Complete, bytes: &[0; 8192],
    };
    let mut bytes = [0; 8256];
    encode_readback_receipt(receipt, &mut bytes).unwrap();
    let header = readback_header(&bytes[..64]).unwrap();
    assert_eq!(header.payload_bytes, 8192);
    assert_eq!(header.source, receipt.source);
    assert_eq!(readback_header(&bytes[..63]), Err(CodecError::Length));
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(readback_header(&bytes[..64]), Err(CodecError::Length));
    bytes[12..16].copy_from_slice(&8192u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(readback_header(&bytes[..64]), Err(CodecError::InvalidCommand));
}
