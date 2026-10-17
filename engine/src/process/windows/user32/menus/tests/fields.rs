use super::*;

const INFO: u32 = 0x10800;
const OUTPUT: u32 = 0x10900;
const TEXT: u32 = 0x10a00;

fn write_words(engine: &mut EngineInstance, address: u32, words: &[u32]) {
    let bytes: Vec<u8> = words.iter().copied().flat_map(u32::to_le_bytes).collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &bytes)
        .unwrap();
}

fn read_words<const N: usize>(engine: &EngineInstance, address: u32) -> [u32; N] {
    crate::process::windows::user32::memory::words(engine.memory.as_ref().unwrap(), address)
        .unwrap()
}

#[test]
fn native_missing_insert_positions_and_commands_append_on_an_empty_menu() {
    for api in [0x0002_0082, 0x0002_0083] {
        for flags in [0, 0x400] {
            for position in [0, 1, 99, u32::MAX] {
                let mut engine = engine();
                let menu = call(&mut engine, 0x0002_0077, &[]).unwrap();
                assert_eq!(
                    call(&mut engine, api, &[menu, position, flags, 37, 0]),
                    Ok(1)
                );
                assert_eq!(call(&mut engine, 0x0002_007d, &[menu]), Ok(1));
                assert_eq!(call(&mut engine, 0x0002_007e, &[menu, 0]), Ok(37));
            }
        }
    }
}

#[test]
fn native_item_info_zero_and_short_capacities_publish_exact_count_and_terminator() {
    for (capacity, count, expected) in [
        (0, 6, [0xeeee, 0xeeee, 0xeeee, 0xeeee]),
        (1, 0, [0, 0xeeee, 0xeeee, 0xeeee]),
        (3, 2, [97, 98, 0, 0xeeee]),
        (16, 6, [97, 98, 99, 100]),
    ] {
        let mut engine = engine();
        let text: Vec<u8> = "abcdef\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(TEXT), &text)
            .unwrap();
        let menu = call(&mut engine, 0x0002_0077, &[]).unwrap();
        assert_eq!(call(&mut engine, 0x0002_0081, &[menu, 0, 37, TEXT]), Ok(1));
        write_words(
            &mut engine,
            INFO,
            &[48, 0x40, 0, 0, 0, 0, 0, 0, 0, OUTPUT, capacity, 0],
        );
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(OUTPUT), &[0xee; 32])
            .unwrap();
        assert_eq!(call(&mut engine, 0x0002_008f, &[menu, 0, 1, INFO]), Ok(1));
        assert_eq!(read_words::<12>(&engine, INFO)[10], count);
        let mut bytes = [0; 8];
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(OUTPUT), &mut bytes)
            .unwrap();
        assert_eq!(
            core::array::from_fn::<_, 4, _>(|index| u16::from_le_bytes([
                bytes[index * 2],
                bytes[index * 2 + 1]
            ])),
            expected
        );
    }
}

#[test]
fn native_owner_draw_payload_is_opaque_item_data_and_never_a_text_pointer() {
    let mut engine = engine();
    let menu = call(&mut engine, 0x0002_0077, &[]).unwrap();
    assert_eq!(
        call(&mut engine, 0x0002_0081, &[menu, 0x100, 37, 0x12345678]),
        Ok(1)
    );
    write_words(
        &mut engine,
        INFO,
        &[48, 0x30, 0, 0, 0, 0, 0, 0, 0, OUTPUT, 16, 0],
    );
    assert_eq!(call(&mut engine, 0x0002_008f, &[menu, 0, 1, INFO]), Ok(1));
    let fields = read_words::<12>(&engine, INFO);
    assert_eq!(
        (fields[2], fields[8], fields[9], fields[10]),
        (0x100, 0x12345678, 0, 0)
    );
}
