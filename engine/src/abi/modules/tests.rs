use super::*;
use crate::memory::GuestAddress;

#[test]
fn pending_module_record_has_literal_u64_identities_and_fixed_reserved_tail() {
    let record = ModuleRecord32 {
        outcome: 1,
        token: 0x11223344_55667788,
        module: 0x99aabbcc_ddeeff01,
        entry: 0x10001000,
        return_pc: 0x7f002000,
        return_id: 0x30001,
        return_esp: 0x7e004000,
        reason: 1,
        kind: 2,
        ..Default::default()
    };
    let expected = [
        0x4f4d3352, 0x00010001, 72, 0, 1, 0, 0x55667788, 0x11223344, 0xddeeff01, 0x99aabbcc,
        0x10001000, 0x7f002000, 0x30001, 0x7e004000, 1, 2, 0, 0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    let mut output = [0xa5; MODULE_RECORD_SIZE];
    encode_module(&record, &mut output).unwrap();
    assert_eq!(output.as_slice(), expected);
    assert_eq!(decode_module(&expected), Ok(record));
    let mut malformed = output;
    malformed[68] = 1;
    assert_eq!(decode_module(&malformed), Err(AbiError::Reserved));
    malformed = output;
    malformed[60] = 3;
    assert_eq!(decode_module(&malformed), Err(AbiError::Module));
    let mut unchanged = [0xa5; MODULE_RECORD_SIZE];
    assert_eq!(
        encode_module(
            &ModuleRecord32 {
                token: 1,
                ..Default::default()
            },
            &mut unchanged
        ),
        Err(AbiError::Module)
    );
    assert_eq!(unchanged, [0xa5; MODULE_RECORD_SIZE]);
}

#[test]
fn completion_variants_and_gates_keep_exact_bytes_and_reject_ambiguous_entries() {
    for (outcome, value) in [(0, 0), (2, 0x400000), (3, 0), (4, 0), (5, 0x10001000)] {
        let record = ModuleRecord32 {
            outcome,
            value,
            ..Default::default()
        };
        let mut output = [0xa5; MODULE_RECORD_SIZE];
        encode_module(&record, &mut output).unwrap();
        assert_eq!(decode_module(&output), Ok(record));
        assert_eq!(&output[24..], &[0; 48]);
    }
    let gates = [
        GateSpec {
            entry: GuestAddress(0x70000000),
            id: 0x1000a,
        },
        GateSpec {
            entry: GuestAddress(0x7f002000),
            id: 0x30001,
        },
    ];
    let expected = [
        0x474d3352, 0x10001, 40, 0, 2, 0, 0x70000000, 0x1000a, 0x7f002000, 0x30001,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    let mut output = [0xa5; 40];
    encode_module_gates(&gates, &mut output).unwrap();
    assert_eq!(output.as_slice(), expected);
    assert_eq!(
        encode_module_gates(&[gates[1], gates[0]], &mut output),
        Err(AbiError::Module)
    );
    assert_eq!(output.as_slice(), expected);
}
