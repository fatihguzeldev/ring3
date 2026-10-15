use super::*;
#[test]
fn duplicate_shares_pcm_but_controls_and_consumed_cursors_are_independent() {
    let mut audio = audio();
    let mut device = Device::new(true, 48_000);
    device.set_cooperative_level(1, 1).unwrap();
    let source = Buffer::create(
        &mut audio,
        &device,
        BufferDescription {
            flags: BufferDescription::VOLUME,
            bytes: 4,
            format: Some(format()),
            algorithm: [0; 16],
        },
    )
    .unwrap();
    let mut duplicate = source.duplicate(&mut audio).unwrap();
    assert_eq!(source.storage, duplicate.storage);
    assert_ne!(source.voice, duplicate.voice);
    duplicate.volume = -10_000;
    source.apply_controls(&mut audio, &device).unwrap();
    duplicate.apply_controls(&mut audio, &device).unwrap();
    audio
        .write_stream_buffer(source.storage, 0, &[192; 4], &[])
        .unwrap();
    source.play(&mut audio, 0, 0, 1).unwrap();
    duplicate.play(&mut audio, 0, 0, 1).unwrap();
    audio.seek_stream(duplicate.voice, 2).unwrap();
    let packet = audio.submit(1).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap(), &[0.5, 0.5]);
    audio.acknowledge(packet, 1).unwrap();
    assert_eq!(source.positions(&audio).unwrap().0, 1);
    assert_eq!(duplicate.positions(&audio).unwrap().0, 3);
    audio.release_stream_voice(source.voice).unwrap();
    assert!(audio.release_stream_buffer(source.storage).is_err());
    audio.release_stream_voice(duplicate.voice).unwrap();
    audio.release_stream_buffer(source.storage).unwrap();
}
#[test]
fn literal_wave_format_and_invalid_capabilities_are_not_silent_success() {
    let encoded = format().encode();
    assert_eq!(
        &encoded[..18],
        &[1, 0, 1, 0, 128, 187, 0, 0, 128, 187, 0, 0, 1, 0, 8, 0, 0, 0]
    );
    assert_eq!(WaveFormat::parse(&encoded[..18]), Ok(format()));
    let desc = BufferDescription {
        flags: BufferDescription::HARDWARE,
        bytes: 4,
        format: Some(format()),
        algorithm: [0; 16],
    };
    assert_eq!(desc.validate(true), Err(HResult::CONTROL_UNAVAILABLE));
    assert_eq!(
        BufferDescription {
            flags: BufferDescription::EFFECTS,
            ..desc
        }
        .validate(true),
        Err(HResult::FX_UNAVAILABLE)
    );
    assert_eq!(
        InterfaceKind::CaptureBuffer8.iid().0[..4],
        [0xf4, 0x0d, 0x99, 0]
    );
    assert_eq!(
        MethodGate {
            interface: InterfaceKind::Buffer8,
            slot: 11
        }
        .method()
        .stack_words,
        8
    );
}
