use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{
            INTERRUPT_DESCRIPTOR_TABLE_SIZE, decode_interrupt_descriptor_table,
            encode_interrupt_descriptor_table,
        },
    },
    cpu::x86::{
        DescriptorTableRegister32, InterruptDescriptorTable32, SegmentDescriptorRecord,
        UserSystemRegisterBank32,
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x4944_5441_5554_4831;
fn table(limit: u16) -> InterruptDescriptorTable32 {
    InterruptDescriptorTable32::new(
        limit,
        Box::new(std::array::from_fn(|vector| {
            SegmentDescriptorRecord::from_words(0xfedc_0000 | vector as u32, 0xffff_ffff)
        })),
    )
}
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for page in [0x1000, 0x3000] {
        engine.map(page, 1, 7).unwrap();
    }
    engine
}
fn snapshot(engine: &EngineInstance) -> (Vec<u8>, [Vec<u8>; 2], u32) {
    let ram = [0x1000, 0x3000].map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    });
    (engine.arena().to_vec(), ram, engine.generation())
}

#[test]
fn complete_raw_authority_has_literal_codec_and_configures_once_without_query_coupling() {
    let mut bytes = [0xa5; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
    encode_interrupt_descriptor_table(&table(0), &mut bytes).unwrap();
    assert_eq!(
        &bytes[..20],
        &[
            82, 51, 73, 68, 1, 0, 1, 0, 20, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(&bytes[20..28], &[0, 0, 0xdc, 0xfe, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(
        &bytes[2060..],
        &[0xff, 0, 0xdc, 0xfe, 0xff, 0xff, 0xff, 0xff]
    );
    let decoded = decode_interrupt_descriptor_table(&bytes).unwrap();
    assert_eq!(decoded.limit(), 0);
    let mut engine = engine();
    engine
        .configure_system_register_bank(UserSystemRegisterBank32 {
            idtr: Some(DescriptorTableRegister32 {
                base: u32::MAX,
                limit: u16::MAX,
            }),
            ..Default::default()
        })
        .unwrap();
    let before = snapshot(&engine);
    engine
        .configure_interrupt_descriptor_table(decoded)
        .unwrap();
    assert_eq!(snapshot(&engine), before);
    assert_eq!(
        engine.configure_interrupt_descriptor_table(table(u16::MAX)),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(snapshot(&engine), before);
    engine.close();
    assert_eq!(
        engine.configure_interrupt_descriptor_table(table(u16::MAX)),
        Err(HostError::Closed)
    );
}

#[test]
fn both_material_owners_freeze_configuration_and_keep_exact_old_artifact_and_ram() {
    for resident in [false, true] {
        let mut engine = engine();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        for (index, value) in [0x1000_u32, 2].into_iter().enumerate() {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let id = if resident {
            Some(engine.compile_resident(1).unwrap())
        } else {
            engine.compile(1).unwrap();
            None
        };
        let material = id.map_or_else(
            || engine.artifact_bytes().unwrap().to_vec(),
            |id| engine.resident_bytes(id.get()).unwrap().to_vec(),
        );
        let before = snapshot(&engine);
        assert_eq!(
            engine.configure_interrupt_descriptor_table(table(7)),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(snapshot(&engine), before);
        let after = id.map_or_else(
            || engine.artifact_bytes().unwrap(),
            |id| engine.resident_bytes(id.get()).unwrap(),
        );
        assert_eq!(after, material);
    }
}
