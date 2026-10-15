use ring3_engine::{
    audio::{AudioError32, PcmFormat32},
    graphics::GraphicsError,
    input::{InputError, InputKind, TimedInputEvent},
    process::{EngineInstance, HostError, MediaConfig, MediaError},
};

fn engine(key: u64) -> EngineInstance {
    let mut engine = EngineInstance::new(4, key).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 100).unwrap();
    engine.observe_cpu_clock(200).unwrap();
    engine
}

fn vertices() -> Vec<u8> {
    [(-1.0_f32, -1.0_f32), (1.0, -1.0), (0.0, 1.0)]
        .into_iter()
        .flat_map(|(x, y)| {
            [
                x.to_le_bytes(),
                y.to_le_bytes(),
                0.0_f32.to_le_bytes(),
                1.0_f32.to_le_bytes(),
                [255, 0, 0, 255],
            ]
            .concat()
        })
        .collect()
}

#[test]
fn configuration_requires_clock_and_publishes_all_owners_together() {
    let mut raw = EngineInstance::new(4, 1).unwrap();
    assert_eq!(
        raw.configure_media(MediaConfig::default()),
        Err(HostError::Infrastructure)
    );
    assert_eq!(
        raw.media_epoch(),
        Err(HostError::Media(MediaError::NotConfigured))
    );
    let mut engine = engine(2);
    let before = engine.arena().to_vec();
    let mut invalid = MediaConfig::default();
    invalid.audio.sample_rate = 0;
    assert_eq!(
        engine.configure_media(invalid),
        Err(HostError::Media(MediaError::Audio(
            AudioError32::InvalidConfig
        )))
    );
    assert!(matches!(
        engine.graphics(),
        Err(HostError::Media(MediaError::NotConfigured))
    ));
    assert!(matches!(
        engine.input(),
        Err(HostError::Media(MediaError::NotConfigured))
    ));
    let epoch = engine.configure_media(MediaConfig::default()).unwrap();
    assert_eq!(engine.graphics().unwrap().epoch(), epoch);
    assert_eq!(engine.audio().unwrap().state().epoch, epoch);
    assert_eq!(engine.input().unwrap().session_epoch(), epoch);
    assert_eq!(
        engine.configure_media(MediaConfig::default()),
        Err(HostError::Media(MediaError::AlreadyConfigured))
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn reset_cancels_three_owners_and_rejects_old_identities() {
    let mut engine = engine(3);
    let epoch = engine.configure_media(MediaConfig::default()).unwrap();
    let resource = engine.graphics_create_vertex_buffer(&vertices()).unwrap();
    engine.graphics_draw_triangles(resource, 0, 3).unwrap();
    let batch = engine.graphics_seal_batch().unwrap().unwrap();
    let buffer = engine
        .audio_create_buffer(
            PcmFormat32 {
                sample_rate: 48000,
                channels: 1,
                bits_per_sample: 8,
            },
            4,
        )
        .unwrap();
    let voice = engine.audio_create_voice(buffer).unwrap();
    engine.audio_play(voice, true).unwrap();
    let submission = engine.audio_submit(2).unwrap();
    engine
        .submit_input(
            epoch,
            1,
            &[
                TimedInputEvent {
                    timestamp_ns: 150,
                    kind: InputKind::Focus { focused: true },
                },
                TimedInputEvent {
                    timestamp_ns: 151,
                    kind: InputKind::Key {
                        code: 20,
                        pressed: true,
                    },
                },
            ],
        )
        .unwrap();
    let lease = engine.input_acquire(false).unwrap();
    let next = engine.reset_media().unwrap();
    assert!(next > epoch);
    assert_eq!(
        engine.graphics_acknowledge_batch(batch),
        Err(HostError::Media(MediaError::Graphics(
            GraphicsError::StaleBatch
        )))
    );
    assert_eq!(
        engine.graphics_draw_triangles(resource, 0, 3),
        Err(HostError::Media(MediaError::Graphics(
            GraphicsError::StaleResource
        )))
    );
    assert_eq!(
        engine.audio_acknowledge(submission, 2),
        Err(HostError::Media(MediaError::Audio(
            AudioError32::InvalidSubmission
        )))
    );
    assert_eq!(
        engine.audio_play(voice, false),
        Err(HostError::Media(MediaError::Audio(
            AudioError32::InvalidVoice
        )))
    );
    assert!(!engine.input().unwrap().lease_valid(lease));
    assert!(!engine.input().unwrap().state().keys[20]);
    assert_eq!(engine.input().unwrap().queued_events(), 0);
    assert_eq!(
        engine.submit_input(
            epoch,
            1,
            &[TimedInputEvent {
                timestamp_ns: 200,
                kind: InputKind::Focus { focused: true }
            }]
        ),
        Err(HostError::Media(MediaError::Input(
            InputError::StaleSession
        )))
    );
    assert_eq!(
        engine.submit_input(
            next,
            1,
            &[TimedInputEvent {
                timestamp_ns: 199,
                kind: InputKind::Focus { focused: true }
            }]
        ),
        Err(HostError::Media(MediaError::Input(InputError::TimeOrder)))
    );
}

#[test]
fn input_uses_observed_engine_time_and_failure_does_not_consume_sequence() {
    let mut engine = engine(4);
    let epoch = engine.configure_media(MediaConfig::default()).unwrap();
    let future = TimedInputEvent {
        timestamp_ns: 201,
        kind: InputKind::Focus { focused: true },
    };
    assert_eq!(
        engine.submit_input(epoch, 1, &[future]),
        Err(HostError::Media(MediaError::Input(InputError::TimeOrder)))
    );
    assert_eq!(engine.input().unwrap().last_sequence(), 0);
    engine.observe_cpu_clock(201).unwrap();
    engine.submit_input(epoch, 1, &[future]).unwrap();
    let mut short = [0xa5; 31];
    assert_eq!(
        engine.input_read_encoded(1, &mut short),
        Err(HostError::Media(MediaError::Input(
            InputError::OutputCapacity
        )))
    );
    assert_eq!(short, [0xa5; 31]);
    assert_eq!(engine.input().unwrap().queued_events(), 1);
}

#[test]
fn oversized_native_input_refuses_count_before_scanning_timestamps() {
    let mut engine = engine(8);
    let mut config = MediaConfig::default();
    config.input.batch_events = 2;
    let epoch = engine.configure_media(config).unwrap();
    let before = (
        *engine.input().unwrap().state(),
        engine.input().unwrap().last_sequence(),
        engine.input().unwrap().last_timestamp_ns(),
        engine.input().unwrap().queued_events(),
        engine.arena().to_vec(),
    );
    let oversized = [TimedInputEvent {
        timestamp_ns: u64::MAX,
        kind: InputKind::Focus { focused: true },
    }; 3];
    assert_eq!(
        engine.submit_input(epoch, 1, &oversized),
        Err(HostError::Media(MediaError::Input(
            InputError::InvalidCount
        )))
    );
    assert_eq!(
        (
            *engine.input().unwrap().state(),
            engine.input().unwrap().last_sequence(),
            engine.input().unwrap().last_timestamp_ns(),
            engine.input().unwrap().queued_events(),
            engine.arena().to_vec(),
        ),
        before
    );

    let valid = [
        TimedInputEvent {
            timestamp_ns: 201,
            kind: InputKind::Focus { focused: true },
        },
        TimedInputEvent {
            timestamp_ns: 201,
            kind: InputKind::Capture { captured: true },
        },
    ];
    assert_eq!(
        engine.submit_input(epoch, 1, &valid),
        Err(HostError::Media(MediaError::Input(InputError::TimeOrder)))
    );
    engine.observe_cpu_clock(201).unwrap();
    engine.submit_input(epoch, 1, &valid).unwrap();
    assert_eq!(engine.input().unwrap().last_sequence(), 1);
    assert_eq!(engine.input().unwrap().queued_events(), 2);
}

#[test]
fn distinct_instances_cannot_accept_each_others_resources() {
    let mut first = engine(5);
    let mut second = engine(6);
    first.configure_media(MediaConfig::default()).unwrap();
    second.configure_media(MediaConfig::default()).unwrap();
    let one = first.graphics_create_vertex_buffer(&vertices()).unwrap();
    let two = second.graphics_create_vertex_buffer(&vertices()).unwrap();
    assert_ne!(one.epoch, two.epoch);
    assert_eq!(
        second.graphics_draw_triangles(one, 0, 3),
        Err(HostError::Media(MediaError::Graphics(
            GraphicsError::StaleResource
        )))
    );
    second.graphics_draw_triangles(two, 0, 3).unwrap();
}

#[test]
fn close_releases_contexts_and_all_public_entrypoints_fail_closed() {
    let mut engine = engine(7);
    engine.configure_media(MediaConfig::default()).unwrap();
    engine.close();
    assert_eq!(engine.reset_media(), Err(HostError::Closed));
    assert_eq!(
        engine.configure_media(MediaConfig::default()),
        Err(HostError::Closed)
    );
    assert!(matches!(engine.graphics(), Err(HostError::Closed)));
    assert!(matches!(engine.audio(), Err(HostError::Closed)));
    assert!(matches!(engine.input(), Err(HostError::Closed)));
    assert_eq!(engine.graphics_present(1), Err(HostError::Closed));
    assert_eq!(engine.audio_submit(1), Err(HostError::Closed));
    assert_eq!(engine.submit_input(1, 1, &[]), Err(HostError::Closed));
    assert_eq!(engine.sync_windows_input(), Err(HostError::Closed));
    assert_eq!(
        engine.submit_windows_input(1, 1, &[]),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.register_input_controller(
            0,
            ring3_engine::input::dinput::ControllerDescriptor32 {
                instance: [0; 16],
                product: [0; 16],
                name: Vec::new(),
                axes: 0,
                povs: 0,
                buttons: 0,
                kind: ring3_engine::input::dinput::ControllerKind32::Gamepad,
            }
        ),
        Err(HostError::Closed)
    );
}

#[test]
fn unconfigured_input_delivery_preserves_lazy_media_admission_without_a_clock() {
    let mut engine = EngineInstance::new(4, 9).unwrap();
    assert_eq!(engine.sync_windows_input(), Ok(()));
    let missing = HostError::Media(ring3_engine::process::MediaError::NotConfigured);
    assert_eq!(engine.submit_input(1, 1, &[]), Err(missing));
    assert_eq!(engine.submit_windows_input(1, 1, &[]), Err(missing));
}
