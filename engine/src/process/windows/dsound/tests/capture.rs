use super::support::{DATA, context, read};
use crate::{
    audio::{
        AudioConfig32, AudioContext, PcmFormat32, SampleEncoding32,
        capture_effects::{CaptureEffectKind as K, class_id, instance_id},
        dsound::{HResult, InterfaceKind, WaveFormat},
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::dsound::{DSoundRuntime32, memory::write_words, objects::ObjectKind32},
    windows::window::{WindowContext32, WindowDescription32, WindowExtent32},
};

fn definitions(runtime: &mut DSoundRuntime32, memory: &mut AddressSpace) -> WindowContext32 {
    runtime.config.max_capture_bytes = 2 * 1024 * 1024;
    let format = WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 32,
        },
        encoding: SampleEncoding32::Float,
        extensible: false,
        channel_mask: 0,
    };
    memory
        .write(GuestAddress(DATA + 64), &format.encode()[..18])
        .unwrap();
    write_words(memory, DATA + 128, &[36, 0, 4096, 0, DATA + 64, 0, 0, 0, 0]).unwrap();
    write_words(
        memory,
        DATA + 256,
        &[28, 0x200, 4096, 0, DATA + 64, 2, DATA + 768],
    )
    .unwrap();
    for (index, kind) in [K::Aec, K::NoiseSuppress].into_iter().enumerate() {
        let address = DATA + 768 + index as u32 * 48;
        write_words(memory, address, &[48, 2]).unwrap();
        memory
            .write(GuestAddress(address + 8), &class_id(kind))
            .unwrap();
        memory
            .write(GuestAddress(address + 24), &instance_id(kind, true))
            .unwrap();
    }
    let mut windows = WindowContext32::new(1, 8).unwrap();
    windows
        .create(WindowDescription32 {
            thread_id: 1,
            procedure: GuestAddress(0x9000),
            extent: WindowExtent32 {
                width: 1,
                height: 1,
            },
            visible: true,
            enabled: true,
        })
        .unwrap();
    windows
}
fn audio() -> AudioContext {
    AudioContext::new(
        7,
        AudioConfig32 {
            sample_rate: 8000,
            max_buffer_bytes: 2 * 1024 * 1024,
            render_frames: 256,
            ..Default::default()
        },
    )
    .unwrap()
}
fn duplex(
    runtime: &mut DSoundRuntime32,
    memory: &mut AddressSpace,
    audio: &mut AudioContext,
    windows: &WindowContext32,
) {
    assert_eq!(
        runtime.create_full_duplex(
            memory,
            audio,
            Some(windows),
            &[
                0,
                0,
                DATA + 256,
                DATA + 128,
                1,
                1,
                DATA + 600,
                DATA + 604,
                DATA + 608,
                0
            ]
        ),
        Ok(0)
    );
}
#[test]
fn full_duplex_capture_effects_have_real_com_controls_status_and_reference_lifetime() {
    let (mut runtime, mut memory, _, _) = context();
    let windows = definitions(&mut runtime, &mut memory);
    let mut audio = audio();
    duplex(&mut runtime, &mut memory, &mut audio, &windows);
    let capture = GuestAddress(read(&memory, DATA + 604));
    let render = GuestAddress(read(&memory, DATA + 608));
    let duplex = GuestAddress(read(&memory, DATA + 600));
    assert_eq!(runtime.com.references(render).unwrap(), 2);
    runtime
        .capture_buffer_call(
            &mut memory,
            &mut audio,
            None,
            capture,
            13,
            &[capture.0, 2, DATA + 640],
        )
        .unwrap();
    assert_eq!(
        [read(&memory, DATA + 640), read(&memory, DATA + 644)],
        [0x20; 2]
    );
    memory
        .write(GuestAddress(DATA + 680), &class_id(K::Aec))
        .unwrap();
    memory
        .write(
            GuestAddress(DATA + 700),
            &InterfaceKind::CaptureFxAec.iid().0,
        )
        .unwrap();
    runtime
        .capture_buffer_call(
            &mut memory,
            &mut audio,
            None,
            capture,
            12,
            &[capture.0, DATA + 680, 0, DATA + 700, DATA + 620],
        )
        .unwrap();
    let effect = GuestAddress(read(&memory, DATA + 620));
    write_words(&mut memory, DATA + 720, &[1, 1, 0xdeadbeef]).unwrap();
    runtime
        .capture_effect_call(
            &mut memory,
            effect,
            InterfaceKind::CaptureFxAec,
            3,
            &[effect.0, DATA + 720],
        )
        .unwrap();
    runtime
        .capture_effect_call(
            &mut memory,
            effect,
            InterfaceKind::CaptureFxAec,
            4,
            &[effect.0, DATA + 720],
        )
        .unwrap();
    assert_eq!(
        [
            read(&memory, DATA + 720),
            read(&memory, DATA + 724),
            read(&memory, DATA + 728)
        ],
        [1, 1, 0]
    );
    let ObjectKind32::Buffer(buffer) = &runtime.object(render).unwrap().kind else {
        unreachable!()
    };
    audio
        .write_stream_buffer(buffer.native.storage, 0, &vec![0; 4096], &[])
        .unwrap();
    runtime
        .buffer_call(
            &mut memory,
            &mut audio,
            None,
            render,
            12,
            &[render.0, 0, 0, 1],
        )
        .unwrap();
    runtime
        .capture_buffer_call(&mut memory, &mut audio, None, capture, 9, &[capture.0, 1])
        .unwrap();
    let submitted = audio.submit(256).unwrap();
    let slot = runtime.slot(capture).unwrap();
    let ObjectKind32::CaptureBuffer(buffer) = &mut runtime.records[slot].as_mut().unwrap().kind
    else {
        unreachable!()
    };
    let mut pcm = vec![0; 1024];
    buffer.process_capture(
        &audio,
        0,
        &mut pcm,
        runtime.capture_aec,
        runtime.capture_noise,
        true,
    );
    assert_eq!(buffer.effects.as_ref().unwrap().mode(), 2);
    runtime
        .capture_effect_call(
            &mut memory,
            effect,
            InterfaceKind::CaptureFxAec,
            6,
            &[effect.0],
        )
        .unwrap();
    runtime
        .capture_effect_call(
            &mut memory,
            effect,
            InterfaceKind::CaptureFxAec,
            5,
            &[effect.0, DATA + 720],
        )
        .unwrap();
    assert_eq!(read(&memory, DATA + 720), 0);
    audio.acknowledge(submitted, 256).unwrap();
    for (pointer, interface) in [
        (render, InterfaceKind::Buffer8),
        (capture, InterfaceKind::CaptureBuffer8),
        (effect, InterfaceKind::CaptureFxAec),
        (duplex, InterfaceKind::FullDuplex),
    ] {
        runtime
            .release(&mut memory, &mut audio, None, pointer, interface)
            .unwrap();
    }
    assert_eq!(runtime.records.iter().flatten().count(), 1);
    assert!(audio.prepare_capture_reference().unwrap().is_some());
}
#[test]
fn refused_capture_effect_location_and_invalid_outputs_do_not_publish_any_resource() {
    let (mut runtime, mut memory, _, _) = context();
    let windows = definitions(&mut runtime, &mut memory);
    let mut audio = audio();
    let count = runtime.records.iter().flatten().count();
    let mapped = memory.mapped_pages();
    write_words(&mut memory, DATA + 600, &[0xdeadbeef; 3]).unwrap();
    write_words(&mut memory, DATA + 772, &[1]).unwrap();
    assert_eq!(
        runtime.create_full_duplex(
            &mut memory,
            &mut audio,
            Some(&windows),
            &[
                0,
                0,
                DATA + 256,
                DATA + 128,
                1,
                1,
                DATA + 600,
                DATA + 604,
                DATA + 608,
                0
            ]
        ),
        Err(HResult::FX_UNAVAILABLE)
    );
    assert_eq!(runtime.records.iter().flatten().count(), count);
    assert_eq!(memory.mapped_pages(), mapped);
    assert_eq!(read(&memory, DATA + 600), 0xdeadbeef);
    write_words(&mut memory, DATA + 772, &[2]).unwrap();
    assert!(
        runtime
            .create_full_duplex(
                &mut memory,
                &mut audio,
                Some(&windows),
                &[
                    0,
                    0,
                    DATA + 256,
                    DATA + 128,
                    1,
                    1,
                    DATA + 600,
                    DATA + 604,
                    0xdead0000,
                    0
                ]
            )
            .is_err()
    );
    assert_eq!(runtime.records.iter().flatten().count(), count);
    assert_eq!(memory.mapped_pages(), mapped);
}
