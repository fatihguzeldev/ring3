use super::*;

struct Arguments32(Vec<u32>);
impl MessageArguments32 for Arguments32 {
    fn word(&self, index: usize) -> Result<u32, MessageError32> {
        self.0
            .get(index)
            .copied()
            .ok_or(MessageError32::InvalidParameter)
    }
    fn character(&self, value: u16, _: bool) -> Result<Vec<u16>, MessageError32> {
        Ok(vec![value])
    }
    fn string(
        &self,
        pointer: u32,
        _: bool,
        precision: Option<usize>,
    ) -> Result<Vec<u16>, MessageError32> {
        let value = match pointer {
            100 => "Ned",
            101 => "Sue",
            _ => return Err(MessageError32::InvalidParameter),
        };
        Ok(value
            .encode_utf16()
            .take(precision.unwrap_or(usize::MAX))
            .collect())
    }
}
fn format(text: &str, flags: u32, words: &[u32]) -> Result<String, MessageError32> {
    let output = format_message(
        &text.encode_utf16().collect::<Vec<_>>(),
        flags,
        true,
        &Arguments32(words.to_vec()),
    )?;
    Ok(String::from_utf16(&output).unwrap())
}
#[test]
fn native_escapes_ignore_inserts_and_width_controls_have_distinct_semantics() {
    assert_eq!(
        format("A\nB\rC\r\nD%nE%rF%tG%%%.%!%b%0tail", 0, &[]).unwrap(),
        "A\r\nB\r\nC\r\nD\r\nE\rF\tG%.! "
    );
    assert_eq!(
        format("[%1!08x!][%2]%%", 0x200, &[]).unwrap(),
        "[%1!08x!][%2]%%"
    );
    assert_eq!(
        format("A%nB%rC%tD%%E%0tail", 0x200, &[]).unwrap(),
        "A\r\nB\rC\tD%%E"
    );
    assert_eq!(
        format("a\r\nb\nc\rd%nend", 255, &[]).unwrap(),
        "a b c d\r\nend"
    );
    assert_eq!(
        format("a bb ccc ddddddd e", 5, &[]).unwrap(),
        "a bb\r\nccc\r\nddddd\r\ndd e"
    );
}
#[test]
fn native_array_and_variable_star_indices_and_i64_word_consumption_are_preserved() {
    let words = [4, 2, 100, 101, 6, 100];
    assert_eq!(
        format("%1!*.*s!/%4/%5!*s!", 0x2000, &words).unwrap(),
        "  Ne/Sue/   Ned"
    );
    assert_eq!(
        format("%1!*.*s!/%3/%4!*s!", 0, &words).unwrap(),
        "  Ne/Sue/   Ned"
    );
    assert_eq!(
        format(
            "%1!I64x!/%2!d!/%3!d!",
            0,
            &[0x55667788, 0x11223344, (-7i32) as u32, 11, 22]
        )
        .unwrap(),
        "1122334455667788/-7/11"
    );
    assert_eq!(
        format(
            "%1!+06d!/%2!#08x!/%3!u!/%4!.2s!",
            0x2000,
            &[(-7i32) as u32, 0x1a, 8, 100]
        )
        .unwrap(),
        "-00007/0x00001a/8/Ne"
    );
}
#[test]
fn malformed_or_excessive_insert_specs_fail_without_unbounded_output() {
    for text in ["%", "%1!", "%1!f!", "%1!65536s!", "%1!I64u!"] {
        assert!(format(text, 0x2000, &[0, 0]).is_err(), "{text}");
    }
}
#[test]
fn native_pointer_case_prefix_and_null_strings_are_distinct_from_integer_hex() {
    assert_eq!(
        format("%1!p!/%1!#p!/[%2!s!]", 0x2000, &[0x1abc, 0]).unwrap(),
        "00001ABC/0X00001ABC/[(null)]"
    );
    assert_eq!(
        format("[%1!foo", 0x200, &[]),
        Err(MessageError32::InvalidParameter)
    );
}
#[test]
fn literal_message_tables_preserve_encoded_payload_and_reject_all_truncations() {
    let bytes = [
        1, 0, 0, 0, 7, 0, 0, 0, 7, 0, 0, 0, 16, 0, 0, 0, 10, 0, 1, 0, b'a', 0, b'z', 0, 0, 0,
    ];
    let entry = message_table_entry(&bytes, 7).unwrap().unwrap();
    assert!(entry.wide);
    assert_eq!(entry.bytes, &[b'a', 0, b'z', 0]);
    assert!(message_table_entry(&bytes, 8).unwrap().is_none());
    for end in 0..bytes.len() {
        assert!(message_table_entry(&bytes[..end], 7).is_err(), "{end}");
    }
    let mut invalid = bytes;
    invalid[18] = 2;
    assert!(message_table_entry(&invalid, 7).is_err());
}

#[test]
fn message_blocks_can_share_payloads_and_need_no_identifier_ordering() {
    let bytes = [
        2, 0, 0, 0, 9, 0, 0, 0, 9, 0, 0, 0, 28, 0, 0, 0, 7, 0, 0, 0, 7, 0, 0, 0, 28, 0, 0, 0, 6, 0,
        0, 0, b'a', 0,
    ];
    for id in [7, 9] {
        assert_eq!(
            message_table_entry(&bytes, id).unwrap().unwrap().bytes,
            b"a"
        );
    }
    assert!(message_table_entry(&bytes, 8).unwrap().is_none());
}
