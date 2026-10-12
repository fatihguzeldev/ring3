use ring3_engine::cpu::x86::State32;
use ring3_engine::memory::{GuestAddress, WordWrite32};
use ring3_engine::windows::{CallbackFrame32, FrameError};

fn state(esp: u32) -> State32 {
    State32 {
        registers: [
            0x1122_3344,
            0xfedc_ba98,
            0x2233_4455,
            0x3344_5566,
            esp,
            0x4455_6677,
            0x5566_7788,
            0x6677_8899,
        ],
        eip: 0x1234_5678,
        eflags: 0xcd7,
    }
}

fn word(address: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(address),
        value,
    }
}

#[test]
fn zero_argument_callback_plan_preserves_gprs_and_flags_and_prepends_return() {
    let original = state(0x9000);
    let frame = CallbackFrame32::prepare(original, 0x2000, 0x1133_5577, &[]).unwrap();
    let mut expected = original;
    expected.eip = 0x2000;
    expected.registers[4] = 0x8ffc;
    assert_eq!(frame.state(), &expected);
    assert_eq!(frame.return_esp(), 0x9000);
    assert_eq!(frame.writes(), &[word(0x8ffc, 0x1133_5577)]);
    assert_eq!(original, state(0x9000));
}

#[test]
fn one_argument_follows_the_return_word_in_logical_order() {
    let original = state(0x9000);
    let frame = CallbackFrame32::prepare(original, 0x2000, 0x3000, &[0xdead_beef]).unwrap();
    let mut expected = original;
    expected.eip = 0x2000;
    expected.registers[4] = 0x8ff8;
    assert_eq!(frame.state(), &expected);
    assert_eq!(frame.return_esp(), 0x9000);
    assert_eq!(
        frame.writes(),
        &[word(0x8ff8, 0x3000), word(0x8ffc, 0xdead_beef)]
    );
}

#[test]
fn sixteen_arguments_use_all_seventeen_descriptors_without_reversing_values() {
    let arguments = [
        0,
        u32::MAX,
        0x8000_0000,
        0x7fff_ffff,
        4,
        5,
        6,
        7,
        8,
        9,
        10,
        11,
        12,
        13,
        14,
        0xcafe_babe,
    ];
    let original = state(0x9000);
    let frame = CallbackFrame32::prepare(original, 0x2000, 0x3000, &arguments).unwrap();
    let mut expected = original;
    expected.eip = 0x2000;
    expected.registers[4] = 0x8fbc;
    assert_eq!(frame.state(), &expected);
    assert_eq!(frame.return_esp(), 0x9000);
    assert_eq!(frame.writes().len(), 17);
    assert_eq!(frame.writes()[0], word(0x8fbc, 0x3000));
    for (index, value) in arguments.iter().enumerate() {
        assert_eq!(
            frame.writes()[index + 1],
            word(0x8fc0 + index as u32 * 4, *value)
        );
    }
    assert_eq!(frame.writes()[16], word(0x8ffc, 0xcafe_babe));
}

#[test]
fn explicit_word_addresses_wrap_from_the_final_return_word_to_zero() {
    let original = state(8);
    let frame =
        CallbackFrame32::prepare(original, 0x2000, 0x3000, &[0x1122_3344, 0x5566_7788]).unwrap();
    let mut expected = original;
    expected.eip = 0x2000;
    expected.registers[4] = 0xffff_fffc;
    assert_eq!(frame.state(), &expected);
    assert_eq!(frame.return_esp(), 8);
    assert_eq!(
        frame.writes(),
        &[
            word(0xffff_fffc, 0x3000),
            word(0, 0x1122_3344),
            word(4, 0x5566_7788)
        ]
    );
}

#[test]
fn unaligned_plans_keep_the_exact_esp_instead_of_aligning_stack_words() {
    let original = state(0x9001);
    let frame = CallbackFrame32::prepare(original, 0x2000, 0x3000, &[0x1122_3344]).unwrap();
    assert_eq!(frame.state().registers[4], 0x8ff9);
    assert_eq!(frame.return_esp(), 0x9001);
    assert_eq!(
        frame.writes(),
        &[word(0x8ff9, 0x3000), word(0x8ffd, 0x1122_3344)]
    );
}

#[test]
fn plan_preparation_preserves_overflowing_word_addresses_for_memory_validation() {
    let original = state(1);
    let frame = CallbackFrame32::prepare(original, 0, u32::MAX, &[]).unwrap();
    assert_eq!(frame.state().eip, 0);
    assert_eq!(frame.state().registers[4], 0xffff_fffd);
    assert_eq!(frame.return_esp(), 1);
    assert_eq!(frame.writes(), &[word(0xffff_fffd, u32::MAX)]);
}

#[test]
fn seventeen_arguments_are_rejected_before_constructing_a_frame() {
    let original = state(0);
    assert_eq!(
        CallbackFrame32::prepare(original, 0x2000, 0x3000, &[0; 17]),
        Err(FrameError::InvalidRequest)
    );
    assert_eq!(original, state(0));
}

#[test]
fn prepared_descriptors_own_their_argument_values() {
    let mut arguments = [0xdead_beef, 0xcafe_babe];
    let frame = CallbackFrame32::prepare(state(0x9000), 0x2000, 0x3000, &arguments).unwrap();
    arguments.fill(0);
    assert_eq!(
        frame.writes(),
        &[
            word(0x8ff4, 0x3000),
            word(0x8ff8, 0xdead_beef),
            word(0x8ffc, 0xcafe_babe)
        ]
    );
}
