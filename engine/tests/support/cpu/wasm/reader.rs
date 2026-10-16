pub(super) struct Reader<'a>(pub(super) &'a [u8]);

impl<'a> Reader<'a> {
    pub(super) fn take(&mut self, length: usize) -> &'a [u8] {
        let (head, tail) = self.0.split_at(length);
        self.0 = tail;
        head
    }
    pub(super) fn byte(&mut self) -> u8 {
        self.take(1)[0]
    }
    pub(super) fn unsigned(&mut self) -> usize {
        let mut value = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = self.byte();
            assert!(shift < 28 || byte & 0xf0 == 0, "invalid u32 LEB128");
            value |= u32::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return value as usize;
            }
        }
        panic!("unterminated u32 LEB128")
    }
    pub(super) fn name(&mut self) -> &'a str {
        let length = self.unsigned();
        std::str::from_utf8(self.take(length)).unwrap()
    }
    pub(super) fn signed_word(&mut self) -> u32 {
        let mut value = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = self.byte();
            value |= u32::from(byte & 127).wrapping_shl(shift);
            if byte & 128 == 0 {
                if shift < 28 && byte & 64 != 0 {
                    value |= u32::MAX << (shift + 7);
                }
                return value;
            }
        }
        panic!("unterminated signed i32 LEB128")
    }
    pub(super) fn end(self) {
        assert!(self.0.is_empty());
    }
}

pub(super) fn section(wasm: &[u8], requested: u8) -> Reader<'_> {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut reader = Reader(&wasm[8..]);
    while !reader.0.is_empty() {
        let kind = reader.byte();
        let length = reader.unsigned();
        let bytes = reader.take(length);
        if kind == requested {
            return Reader(bytes);
        }
    }
    panic!("missing Wasm section {requested}")
}
