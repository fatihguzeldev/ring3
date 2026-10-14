#[path = "../../control/exceptions/support/stopped.rs"]
mod stopped;
use ring3_engine::{
    cpu::x86::decode::{DecodeError, decode_one},
    memory::GuestAddress,
};
fn pairs() -> [(&'static [u8], &'static [u8]); 17] {
    [
        (&[0x82, 0xc0, 0x01], &[0x80, 0xc0, 0x01]),
        (&[0x82, 0xc8, 0x01], &[0x80, 0xc8, 0x01]),
        (&[0x82, 0xd0, 0x01], &[0x80, 0xd0, 0x01]),
        (&[0x82, 0xd8, 0x01], &[0x80, 0xd8, 0x01]),
        (&[0x82, 0xe0, 0x01], &[0x80, 0xe0, 0x01]),
        (&[0x82, 0xe8, 0x01], &[0x80, 0xe8, 0x01]),
        (&[0x82, 0xf0, 0x01], &[0x80, 0xf0, 0x01]),
        (&[0x82, 0xf8, 0x01], &[0x80, 0xf8, 0x01]),
        (&[0xd0, 0xf0], &[0xd0, 0xe0]),
        (&[0xd2, 0xf0], &[0xd2, 0xe0]),
        (&[0xc0, 0xf0, 0x01], &[0xc0, 0xe0, 0x01]),
        (&[0x66, 0xd1, 0xf0], &[0x66, 0xd1, 0xe0]),
        (&[0x66, 0xd3, 0xf0], &[0x66, 0xd3, 0xe0]),
        (&[0x66, 0xc1, 0xf0, 0x01], &[0x66, 0xc1, 0xe0, 0x01]),
        (&[0xd1, 0xf0], &[0xd1, 0xe0]),
        (&[0xd3, 0xf0], &[0xd3, 0xe0]),
        (&[0xc1, 0xf0, 0x01], &[0xc1, 0xe0, 0x01]),
    ]
}
#[test]
fn seventeen_defined_register_aliases_match_existing_canonical_operations_and_prepare() {
    for (alias, canonical) in pairs() {
        let a = stopped::stopped(alias, 0x3000);
        let c = stopped::stopped(canonical, 0x3000);
        let actual = decode_one(a.memory().unwrap(), GuestAddress(0x3000)).unwrap();
        let expected = decode_one(c.memory().unwrap(), GuestAddress(0x3000)).unwrap();
        assert_eq!(actual.operation(), expected.operation());
        assert_eq!(actual.length(), expected.length());
        assert_eq!(actual.next_pc(), expected.next_pc());
        assert!(a.memory().unwrap().is_code_current(actual.code_snapshot()));
        stopped::expect_admitted(alias, 0x3000);
    }
}
#[test]
fn already_supported_locked_group1_and_explicit_refusal_boundaries_remain() {
    for selector in 0..7 {
        let alias = [0xf0, 0x82, selector << 3, 1];
        let canonical = [0xf0, 0x80, selector << 3, 1];
        let a = stopped::stopped(&alias, 0x3000);
        let c = stopped::stopped(&canonical, 0x3000);
        assert_eq!(
            decode_one(a.memory().unwrap(), GuestAddress(0x3000))
                .unwrap()
                .operation(),
            decode_one(c.memory().unwrap(), GuestAddress(0x3000))
                .unwrap()
                .operation()
        );
    }
    for bytes in [&[0xf0, 0x82, 0x38, 1][..], &[0xf0, 0xd1, 0x30]] {
        let e = stopped::stopped(bytes, 0x3000);
        assert_eq!(
            decode_one(e.memory().unwrap(), GuestAddress(0x3000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
    for bytes in [
        &[0x66, 0x66, 0x82, 0xc0, 1][..],
        &[0xf6, 0xc8, 1],
        &[0x66, 0x0f, 0xc8],
    ] {
        let e = stopped::stopped(bytes, 0x3000);
        assert!(matches!(
            decode_one(e.memory().unwrap(), GuestAddress(0x3000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
}

#[test]
fn equal_width_word_moves_use_word_ir_and_original_continuations() {
    for (alias, canonical) in [
        (&[0x66, 0x0f, 0xb7, 0xc1][..], &[0x66, 0x8b, 0xc1][..]),
        (&[0x66, 0x0f, 0xbf, 0xc1][..], &[0x66, 0x8b, 0xc1][..]),
        (&[0x66, 0x0f, 0xb7, 0x03][..], &[0x66, 0x8b, 0x03][..]),
        (&[0x66, 0x0f, 0xbf, 0x03][..], &[0x66, 0x8b, 0x03][..]),
    ] {
        let a = stopped::stopped(alias, 0x3000);
        let c = stopped::stopped(canonical, 0x3000);
        let actual = decode_one(a.memory().unwrap(), GuestAddress(0x3000)).unwrap();
        let want = decode_one(c.memory().unwrap(), GuestAddress(0x3000)).unwrap();
        assert_eq!(actual.operation(), want.operation());
        assert_eq!(usize::from(actual.length()), alias.len());
        assert_eq!(usize::from(want.length()) + 1, usize::from(actual.length()));
        assert_eq!(actual.next_pc(), GuestAddress(0x3000 + alias.len() as u32));
        assert!(a.memory().unwrap().is_code_current(actual.code_snapshot()));
        stopped::expect_admitted(alias, 0x3000);
    }
}
