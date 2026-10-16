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

#[test]
fn virtual_primary_requires_write_primary_for_direct_sample_access() {
    let mut audio = audio();
    let mut device = Device::new(true, 48_000);
    device.set_cooperative_level(1, 4).unwrap();
    assert_eq!(device.caps()[1] & 0x20, 0);
    device.set_cooperative_level(1, 2).unwrap();
    let primary = Buffer::create(
        &mut audio,
        &device,
        BufferDescription {
            flags: BufferDescription::PRIMARY,
            bytes: 0,
            format: None,
            algorithm: [0; 16],
        },
    )
    .unwrap();
    assert_eq!(
        primary.lock_span(&audio, 0, 4, 0, false),
        Err(HResult::INVALID_CALL)
    );
    assert_eq!(
        primary.lock_primary_span(&audio, &device, 0, 4, 0, false),
        Err(HResult::PRIORITY_LEVEL)
    );
    device.set_cooperative_level(1, 4).unwrap();
    assert_eq!(
        primary
            .lock_primary_span(&audio, &device, 0, 4, 0, false)
            .unwrap()
            .first,
        4
    );
}

#[test]
fn default_and_global_focus_controls_respect_foreground_and_window_lifetime() {
    let mut audio = audio();
    let mut device = Device::new(true, 48_000);
    device.set_cooperative_level(1, 1).unwrap();
    device.focused = false;
    let default = Buffer::create(
        &mut audio,
        &device,
        BufferDescription {
            flags: 0,
            bytes: 4,
            format: Some(format()),
            algorithm: [0; 16],
        },
    )
    .unwrap();
    let global = Buffer::create(
        &mut audio,
        &device,
        BufferDescription {
            flags: 0x8000,
            ..default.description
        },
    )
    .unwrap();
    assert_eq!(default.controls(&device).gains, [0.0; 2]);
    assert_eq!(global.controls(&device).gains, [1.0; 2]);
    device.focused = true;
    assert_eq!(default.controls(&device).gains, [1.0; 2]);
    device.window_alive = false;
    assert_eq!(global.controls(&device).gains, [0.0; 2]);
}

#[test]
fn stopped_looping_buffer_does_not_report_looping_status() {
    let mut audio = audio();
    let device = Device::new(true, 48_000);
    let buffer = Buffer::create(
        &mut audio,
        &device,
        BufferDescription {
            flags: 0,
            bytes: 4,
            format: Some(format()),
            algorithm: [0; 16],
        },
    )
    .unwrap();
    buffer.play(&mut audio, 0, 0, 1).unwrap();
    assert_eq!(buffer.status(&audio), Ok(5));
    audio.stop_stream(buffer.voice).unwrap();
    assert_eq!(buffer.status(&audio), Ok(0));
}

#[test]
fn software_spatial_algorithm_profile_reports_literal_fallback_success() {
    for (algorithm, expected) in [
        (0xc241_333fu32, 0),
        (0xc241_3340, 0x0878_000a),
        (0xc241_3342, 0x0878_000a),
    ] {
        let mut guid = [
            0x3f, 0x33, 0x41, 0xc2, 0x1b, 0x1c, 0xd2, 0x11, 0x94, 0xf5, 0, 0xc0, 0x4f, 0xc2, 0x8a,
            0xca,
        ];
        guid[..4].copy_from_slice(&algorithm.to_le_bytes());
        let description = BufferDescription {
            flags: BufferDescription::CONTROL_3D,
            bytes: 4,
            format: Some(format()),
            algorithm: guid,
        };
        let validated = description.validate(true).unwrap();
        assert_eq!(
            validated.spatial_algorithm().unwrap().creation_result().0,
            expected
        );
        assert_eq!(
            BufferDescription {
                flags: 0,
                ..description
            }
            .validate(true),
            Err(HResult::INVALID_PARAM)
        );
    }
    assert_eq!(
        SpatialAlgorithm::from_guid([1; 16]),
        Err(HResult::INVALID_PARAM)
    );
    assert_eq!(
        SpatialAlgorithm::from_guid([0; 16])
            .unwrap()
            .creation_result()
            .0,
        0
    );
}
