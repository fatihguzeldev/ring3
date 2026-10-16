use super::*;
mod environment;
mod handles;
mod modules;

fn api(name: &str) -> ProcessApi32 {
    ProcessApi32::ALL
        .iter()
        .copied()
        .find(|api| api.name() == name)
        .unwrap_or_else(|| panic!("missing process runtime API: {name}"))
}

#[test]
fn encoded_pointers_round_trip_all_bits_without_reusing_another_process_cookie() {
    let mut first = EngineInstance::new(16, 7).unwrap();
    let mut second = EngineInstance::new(16, 8).unwrap();
    let encode = api("EncodePointer");
    let decode = api("DecodePointer");
    for pointer in [0, 1, 0x8000_0000, 0xdead_beef, u32::MAX] {
        let encoded = first
            .execute_process_call(encode, &[pointer])
            .unwrap()
            .value as u32;
        assert_eq!(
            first
                .execute_process_call(decode, &[encoded])
                .unwrap()
                .value,
            u64::from(pointer)
        );
        assert_ne!(
            second
                .execute_process_call(encode, &[pointer])
                .unwrap()
                .value,
            u64::from(encoded)
        );
    }
}

#[test]
fn processor_features_agree_with_the_virtual_cpu_and_refuse_unknown_numbers() {
    let mut engine = EngineInstance::new(16, 7).unwrap();
    let query = api("IsProcessorFeaturePresent");
    for feature in [2, 3, 6, 8, 10, 12, 13, 17, 36, 37, 38, 39, 40] {
        assert_eq!(
            engine
                .execute_process_call(query, &[feature])
                .unwrap()
                .value,
            1,
            "{feature}"
        );
    }
    for feature in [0, 1, 4, 5, 7, 14, 18, 41, 0xffff_ffff] {
        assert_eq!(
            engine
                .execute_process_call(query, &[feature])
                .unwrap()
                .value,
            0,
            "{feature}"
        );
    }
}
