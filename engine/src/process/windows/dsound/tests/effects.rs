use super::support::{DATA, context, read};
use crate::{
    audio::{
        AudioConfig32, AudioContext, PcmFormat32, SampleEncoding32,
        dsound::{
            BufferDescription, HResult as H, InterfaceKind, WaveFormat,
            effects::{class_id, interface_id},
        },
        effects::EffectKind,
    },
    memory::{GuestAddress, Permissions},
    process::windows::dsound::{memory::write_words, objects::ObjectKind32},
};

fn setup() -> (
    super::super::DSoundRuntime32,
    crate::memory::AddressSpace,
    AudioContext,
    GuestAddress,
) {
    let (mut runtime, mut memory, _, device) = context();
    let mut audio = AudioContext::new(
        7,
        AudioConfig32 {
            max_buffer_bytes: 2_000_000,
            render_frames: 16,
            ..Default::default()
        },
    )
    .unwrap();
    let format = WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 48_000,
            channels: 1,
            bits_per_sample: 8,
        },
        encoding: SampleEncoding32::Integer,
        extensible: false,
        channel_mask: 0,
    };
    memory
        .write(GuestAddress(DATA + 64), &format.encode()[..18])
        .unwrap();
    write_words(
        &mut memory,
        DATA + 128,
        &[
            36,
            BufferDescription::EFFECTS,
            7200,
            0,
            DATA + 64,
            0,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 4, 0)
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA + 4));
    (runtime, memory, audio, buffer)
}

fn set(
    runtime: &mut super::super::DSoundRuntime32,
    memory: &mut crate::memory::AddressSpace,
    audio: &mut AudioContext,
    buffer: GuestAddress,
    kind: EffectKind,
) {
    write_words(memory, DATA + 256, &[32, 2, 0, 0, 0, 0, 0, 0]).unwrap();
    memory
        .write(GuestAddress(DATA + 264), &class_id(kind).0)
        .unwrap();
    runtime
        .buffer_effects(
            memory,
            audio,
            buffer,
            21,
            &[buffer.0, 1, DATA + 256, DATA + 320],
        )
        .unwrap();
    assert_eq!(read(memory, DATA + 320), 2);
}

fn object(
    runtime: &mut super::super::DSoundRuntime32,
    memory: &mut crate::memory::AddressSpace,
    audio: &mut AudioContext,
    buffer: GuestAddress,
    kind: EffectKind,
) -> GuestAddress {
    memory
        .write(GuestAddress(DATA + 352), &class_id(kind).0)
        .unwrap();
    memory
        .write(GuestAddress(DATA + 368), &interface_id(kind).0)
        .unwrap();
    runtime
        .buffer_effects(
            memory,
            audio,
            buffer,
            23,
            &[buffer.0, DATA + 352, 0, DATA + 368, DATA + 384],
        )
        .unwrap();
    GuestAddress(read(memory, DATA + 384))
}

#[test]
fn effect_identity_parameters_and_detached_lifetime_survive_chain_replacement() {
    let (mut runtime, mut memory, mut audio, buffer) = setup();
    set(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::Gargle,
    );
    let effect = object(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::Gargle,
    );
    assert_eq!(
        object(
            &mut runtime,
            &mut memory,
            &mut audio,
            buffer,
            EffectKind::Gargle
        ),
        effect
    );
    runtime
        .release(
            &mut memory,
            &mut audio,
            None,
            effect,
            InterfaceKind::FxGargle,
        )
        .unwrap();
    write_words(&mut memory, DATA + 400, &[123, 1]).unwrap();
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxGargle,
            3,
            &[effect.0, DATA + 400],
        )
        .unwrap();
    set(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::Echo,
    );
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxGargle,
            4,
            &[effect.0, DATA + 416],
        )
        .unwrap();
    assert_eq!(
        [read(&memory, DATA + 416), read(&memory, DATA + 420)],
        [123, 1]
    );
    runtime
        .release(&mut memory, &mut audio, None, buffer, InterfaceKind::Buffer)
        .unwrap();
    assert!(runtime.object(buffer).is_ok());
    runtime
        .release(
            &mut memory,
            &mut audio,
            None,
            effect,
            InterfaceKind::FxGargle,
        )
        .unwrap();
    assert!(runtime.object(effect).is_err());
    assert!(runtime.object(buffer).is_err());
}

#[test]
fn refused_effect_output_keeps_old_chain_and_generation_without_pcm_mutation() {
    let (mut runtime, mut memory, mut audio, buffer) = setup();
    set(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::Gargle,
    );
    let ObjectKind32::Buffer(record) = &runtime.object(buffer).unwrap().kind else {
        unreachable!()
    };
    let generation = record.effects_generation;
    let voice = record.native.voice;
    memory
        .protect(
            crate::memory::PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert_eq!(
        runtime.buffer_effects(
            &mut memory,
            &mut audio,
            buffer,
            21,
            &[buffer.0, 1, DATA + 256, DATA + 320]
        ),
        Err(H::INVALID_PARAM)
    );
    let ObjectKind32::Buffer(record) = &runtime.object(buffer).unwrap().kind else {
        unreachable!()
    };
    assert_eq!(record.effects_generation, generation);
    assert_eq!(
        audio
            .voice_effects(voice)
            .unwrap()
            .unwrap()
            .parameters(0)
            .unwrap()
            .kind(),
        EffectKind::Gargle
    );
}

#[test]
fn i3dl2_presets_quality_and_live_pcm_use_the_same_effect_object() {
    let (mut runtime, mut memory, mut audio, buffer) = setup();
    set(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::I3dl2Reverb,
    );
    let effect = object(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::I3dl2Reverb,
    );
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxI3dl2Reverb,
            5,
            &[effect.0, 4],
        )
        .unwrap();
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxI3dl2Reverb,
            6,
            &[effect.0, DATA + 400],
        )
        .unwrap();
    assert_eq!(read(&memory, DATA + 400), 4);
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxI3dl2Reverb,
            7,
            &[effect.0, 3],
        )
        .unwrap();
    runtime
        .effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxI3dl2Reverb,
            8,
            &[effect.0, DATA + 404],
        )
        .unwrap();
    assert_eq!(read(&memory, DATA + 404), 3);
    assert_eq!(
        runtime.effect_call(
            &mut memory,
            &mut audio,
            effect,
            InterfaceKind::FxI3dl2Reverb,
            5,
            &[effect.0, 30]
        ),
        Err(H::INVALID_PARAM)
    );
    set(
        &mut runtime,
        &mut memory,
        &mut audio,
        buffer,
        EffectKind::Gargle,
    );
    let ObjectKind32::Buffer(record) = &runtime.object(buffer).unwrap().kind else {
        unreachable!()
    };
    let voice = record.native.voice;
    let storage = record.native.storage;
    audio
        .write_stream_buffer(storage, 0, &[255; 16], &[])
        .unwrap();
    audio.play_stream(voice, false).unwrap();
    let packet = audio.submit(16).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap()[0], 0.);
    assert!(audio.submission_pcm(packet).unwrap()[30] > 0.);
    assert!(audio.submission_pcm(packet).unwrap()[30] < 127. / 128.);
}
