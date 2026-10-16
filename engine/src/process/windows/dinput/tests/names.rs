use super::{haptics::controller, support::DATA};
use crate::{input::dinput::InterfaceKind32, memory::GuestAddress, windows::text::CodePage32};

#[test]
fn ansi_device_info_uses_the_canonical_code_page_and_preserves_unicode_output() {
    let (mut runtime, mut memory, pointer, _) = controller();
    runtime.ansi = CodePage32::Legacy(932);
    runtime.controllers[0].as_mut().unwrap().name = "日本語".encode_utf16().collect();
    memory
        .write(GuestAddress(DATA + 512), &580u32.to_le_bytes())
        .unwrap();
    runtime
        .device_info(&mut memory, pointer, DATA + 512, InterfaceKind32::Device8A)
        .unwrap();
    let mut output = [0; 580];
    memory.read(GuestAddress(DATA + 512), &mut output).unwrap();
    for at in [40, 300] {
        assert_eq!(
            &output[at..at + 7],
            &[0x93, 0xfa, 0x96, 0x7b, 0x8c, 0xea, 0]
        );
    }
    memory
        .write(GuestAddress(DATA + 512), &1100u32.to_le_bytes())
        .unwrap();
    runtime
        .device_info(&mut memory, pointer, DATA + 512, InterfaceKind32::Device8W)
        .unwrap();
    let mut output = [0; 1100];
    memory.read(GuestAddress(DATA + 512), &mut output).unwrap();
    assert_eq!(&output[40..48], &[0xe5, 0x65, 0x2c, 0x67, 0x9e, 0x8a, 0, 0]);
    runtime.ansi = CodePage32::Windows1252;
    runtime.controllers[0].as_mut().unwrap().name = "€ controller".encode_utf16().collect();
    let output = runtime
        .instance_bytes(
            crate::input::dinput::DeviceKind32::Controller(0),
            false,
            true,
            580,
        )
        .unwrap();
    assert_eq!(&output[40..53], b"\x80 controller\0");
}

#[test]
fn fixed_ansi_fields_truncate_at_complete_dbcs_and_utf8_character_boundaries() {
    let (mut runtime, _, _, _) = controller();
    for (code_page, name, end, last) in [
        (
            CodePage32::Legacy(932),
            "日".repeat(130),
            258,
            vec![0x93, 0xfa],
        ),
        (
            CodePage32::Utf8,
            "😀".repeat(129),
            256,
            vec![0xf0, 0x9f, 0x98, 0x80],
        ),
    ] {
        runtime.ansi = code_page;
        runtime.controllers[0].as_mut().unwrap().name = name.encode_utf16().collect();
        let output = runtime
            .instance_bytes(
                crate::input::dinput::DeviceKind32::Controller(0),
                false,
                true,
                580,
            )
            .unwrap();
        for at in [40, 300] {
            assert_eq!(&output[at + end - last.len()..at + end], &last);
            assert!(output[at + end..at + 260].iter().all(|byte| *byte == 0));
            let decoded = code_page.decode(&output[at..at + end], true).unwrap();
            assert_eq!(
                decoded,
                name.encode_utf16()
                    .take(if end == 258 { 129 } else { 128 })
                    .collect::<Vec<_>>()
            );
        }
    }
}
