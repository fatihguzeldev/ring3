use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::SegmentCache,
    memory::GuestAddress,
    process::EngineInstance,
};
fn result(raw: &[u8], d32: bool) -> bool {
    let mut e = EngineInstance::new(1, 0x424e44).unwrap();
    e.map(0x1000, 1, 7).unwrap();
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + raw.len()].copy_from_slice(raw);
    e.upload(0x1000, raw.len() as u32).unwrap();
    if !d32 {
        let mut c = e.segment_context().unwrap();
        c.caches[1] = SegmentCache::from_bits(c.caches[1].bits() & !0x02000000).unwrap();
        let mut sg = [0; SEGMENTS_SIZE];
        let mut sb = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&c, &mut sg, &mut sb).unwrap();
        e.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
            .copy_from_slice(&sg);
        e.arena_mut().unwrap()
            [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(&sb);
    }
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (raw.len() as u32).to_le_bytes()].concat());
    let before = e.arena().to_vec();
    let mut ram = [0; 4096];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut ram)
        .unwrap();
    let answer = e.compile(1);
    println!("D32={d32}\t{raw:02x?}\t{answer:?}");
    assert_eq!(e.arena(), before);
    let mut after = [0; 4096];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut after)
        .unwrap();
    assert_eq!(ram, after);
    answer.is_ok()
}
fn forms(d32: bool) -> Vec<Vec<u8>> {
    let rel = if d32 { vec![0, 0, 0, 0] } else { vec![0, 0] };
    vec![
        [vec![0xe8], rel.clone()].concat(),
        vec![0xff, 0xd0],
        vec![0xff, 0x10],
        vec![0xc3],
        vec![0xc2, 8, 0],
        [vec![0xe9], rel.clone()].concat(),
        vec![0xff, 0xe0],
        vec![0xff, 0x20],
        vec![0x74, 0],
        [vec![0x0f, 0x84], rel].concat(),
    ]
}
#[test]
fn bnd_prefix_prepares_all_selected_near_families() {
    let mut missing = vec![];
    for d32 in [false, true] {
        for raw in forms(d32) {
            let bytes = [vec![0xf2], raw].concat();
            if !result(&bytes, d32) {
                missing.push((d32, bytes));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "all{} selected prefix preparations refused: {missing:02x?}",
        missing.len()
    );
}
#[test]
fn unprefixed_controls_keep_the_same_lowerers() {
    for d32 in [false, true] {
        for raw in forms(d32) {
            assert!(result(&raw, d32));
        }
    }
}
#[test]
fn excluded_repeat_and_branch_families_stay_host_refusals() {
    for d32 in [false, true] {
        let rel = if d32 { vec![0, 0, 0, 0] } else { vec![0, 0] };
        for raw in [
            vec![0xf2, 0xeb, 0],
            vec![0xf2, 0xe2, 0],
            vec![0xf2, 0xe3, 0],
            vec![0xf2, 0xcb],
            vec![0xf2, 0xcf],
            [vec![0xf3, 0xe9], rel.clone()].concat(),
            [vec![0xf2, 0xf2, 0xe9], rel.clone()].concat(),
            [
                vec![0xf2, 0x66, 0x66, 0xe9],
                if d32 { vec![0, 0] } else { vec![0, 0, 0, 0] },
            ]
            .concat(),
        ] {
            assert!(!result(&raw, d32));
        }
    }
}
