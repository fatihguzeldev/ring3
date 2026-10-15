use super::*;
use crate::graphics::d3d8::api::{ADAPTER_IDENTIFIER_BYTES, CAPS_WORDS};

fn adapter() -> AdapterProfile32 {
    let display = DisplayMode32 { width: 800, height: 600, refresh_rate: 60, format: Format::X8R8G8B8 };
    AdapterProfile32::new(display, vec![display]).unwrap()
}

#[test]
fn desktop_d3d8_normalization_uses_thirteen_words_and_the_actual_client_extent() {
    let source = [0, 0, 0, 0, 0, 1, 0, 7, 9, 75, 0, 0, 0];
    let parameters = PresentationParameters32::decode(source).normalize(42,
        WindowExtent32 { width: 640, height: 480 }, &adapter(), false).unwrap();
    assert_eq!(parameters.words(), [640, 480, 22, 1, 0, 1, 42, 1, 1, 75, 0, 0, 0]);
    assert_eq!(parameters.display_mode(&adapter()), Ok(adapter().display));
    assert_eq!(PRESENT_PARAMETER_WORDS * 4, 52);
}

#[test]
fn swap_effect_sampling_and_fullscreen_mode_constraints_are_distinct() {
    let profile = adapter();
    let extent = WindowExtent32 { width: 800, height: 600 };
    let valid = PresentationParameters32::decode([800,600,22,1,0,1,42,0,0,0,0,60,1]);
    assert!(valid.normalize(42, extent, &profile, false).is_ok());
    assert_eq!(valid.normalize(42, extent, &profile, true), Err(HResult::INVALID_CALL));
    let mut invalid = valid;
    invalid.refresh_rate = 75;
    assert_eq!(invalid.normalize(42, extent, &profile, false), Err(HResult::NOT_AVAILABLE));
    invalid = valid; invalid.swap_effect = 3; invalid.back_buffers = 2;
    assert_eq!(invalid.normalize(42, extent, &profile, false), Err(HResult::INVALID_CALL));
    invalid = valid; invalid.multisample = 2;
    assert_eq!(invalid.normalize(42, extent, &profile, false), Err(HResult::NOT_AVAILABLE));
    invalid.swap_effect = 2;
    assert_eq!(invalid.normalize(42, extent, &profile, false), Err(HResult::INVALID_CALL));
}

#[test]
fn advertised_abi_layout_has_no_d3d9_tail_or_fabricated_whql_certification() {
    let profile = adapter();
    let caps = profile.caps();
    assert_eq!(CAPS_WORDS * 4, 212);
    assert_eq!(&caps[47..], &[16,255,0xfffe0101,96,0xffff0104,8.0f32.to_bits()]);
    assert_eq!(caps[7] & 0x01e0_0000, 0); // optional patch hardware is not advertised.
    let identifier = profile.identifier();
    assert_eq!(ADAPTER_IDENTIFIER_BYTES, 1068);
    assert_eq!(&identifier[..11], b"ring3.d3d8\0");
    assert_eq!(&identifier[1064..], &[0;4]);
}

#[test]
fn processing_modes_and_pure_device_are_validated_before_device_allocation() {
    for flags in [0x20,0x40,0x80,0x56,0x26] { assert_eq!(validate_behavior(flags), Ok(())); }
    for flags in [0,0x60,0xc0,0x30,0x90,0x140] {
        assert_eq!(validate_behavior(flags), Err(HResult::INVALID_CALL));
    }
}

#[test]
fn alpha_backbuffer_matches_an_opaque_fullscreen_display_mode() {
    let profile = adapter();
    let parameters = PresentationParameters32::decode([800,600,21,1,0,1,42,0,0,0,0,60,1]);
    let normalized = parameters.normalize(42, WindowExtent32 { width: 800, height: 600 }, &profile, false).unwrap();
    assert_eq!(normalized.format, 21);
    assert_eq!(normalized.display_mode(&profile).unwrap().format, Format::X8R8G8B8);
    assert_eq!(profile.check_format(crate::graphics::d3d8::resources::Usage(0), 6, Format::VertexData),
        Err(HResult::INVALID_CALL));
}
