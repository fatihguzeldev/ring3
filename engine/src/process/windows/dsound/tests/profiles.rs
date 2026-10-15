use super::{super::memory::write_words, support::*};
use crate::{
    audio::dsound::{BufferDescription, HResult, InterfaceKind},
    memory::GuestAddress,
    windows::window::{WindowContext32, WindowDescription32, WindowExtent32},
};

fn algorithm(first: u32) -> [u8; 16] {
    let mut guid = [
        0x3f, 0x33, 0x41, 0xc2, 0x1b, 0x1c, 0xd2, 0x11, 0x94, 0xf5, 0, 0xc0, 0x4f, 0xc2, 0x8a, 0xca,
    ];
    guid[..4].copy_from_slice(&first.to_le_bytes());
    guid
}

#[test]
fn unavailable_hrtf_publishes_a_real_buffer_with_literal_success_warning() {
    let (mut runtime, mut memory, mut audio, device) = context();
    buffer(
        &mut runtime,
        &mut memory,
        &mut audio,
        device,
        BufferDescription::CONTROL_3D,
    );
    for (guid, result) in [
        ([0; 16], 0),
        (algorithm(0xc241_333f), 0),
        (algorithm(0xc241_3340), 0x0878_000a),
        (algorithm(0xc241_3342), 0x0878_000a),
    ] {
        write_words(
            &mut memory,
            DATA + 128,
            &[36, BufferDescription::CONTROL_3D, 4, 0, DATA + 64],
        )
        .unwrap();
        memory.write(GuestAddress(DATA + 148), &guid).unwrap();
        assert_eq!(
            runtime.create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 8, 0),
            Ok(result)
        );
        let pointer = GuestAddress(read(&memory, DATA + 8));
        assert!(runtime.object(pointer).is_ok());
        runtime
            .release(
                &mut memory,
                &mut audio,
                None,
                pointer,
                InterfaceKind::Buffer8,
            )
            .unwrap();
    }
    let count = runtime.records.iter().flatten().count();
    for (flags, guid) in [
        (0, algorithm(0xc241_3340)),
        (BufferDescription::CONTROL_3D, [1; 16]),
    ] {
        write_words(&mut memory, DATA + 128, &[36, flags, 4, 0, DATA + 64]).unwrap();
        memory.write(GuestAddress(DATA + 148), &guid).unwrap();
        write_words(&mut memory, DATA + 8, &[0xdead_beef]).unwrap();
        assert_eq!(
            runtime.create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 8, 0),
            Err(HResult::INVALID_PARAM)
        );
        assert_eq!(read(&memory, DATA + 8), 0xdead_beef);
        assert_eq!(runtime.records.iter().flatten().count(), count);
    }
}

#[test]
fn full_duplex_returns_the_same_hrtf_fallback_warning_after_atomic_outputs() {
    let (mut runtime, mut memory, mut audio, _) = context();
    memory
        .write(
            GuestAddress(DATA + 64),
            &[1, 0, 1, 0, 128, 187, 0, 0, 128, 187, 0, 0, 1, 0, 8, 0, 0, 0],
        )
        .unwrap();
    write_words(
        &mut memory,
        DATA + 128,
        &[36, BufferDescription::CONTROL_3D, 4, 0, DATA + 64],
    )
    .unwrap();
    memory
        .write(GuestAddress(DATA + 148), &algorithm(0xc241_3340))
        .unwrap();
    write_words(&mut memory, DATA + 256, &[28, 0, 4, 0, DATA + 64, 0, 0]).unwrap();
    let mut windows = WindowContext32::new(1, 8).unwrap();
    let window = windows
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
                window.get(),
                1,
                DATA + 600,
                DATA + 604,
                DATA + 608,
                0
            ]
        ),
        Ok(0x0878_000a)
    );
    for (out, interface) in [
        (DATA + 608, InterfaceKind::Buffer8),
        (DATA + 604, InterfaceKind::CaptureBuffer8),
        (DATA + 600, InterfaceKind::FullDuplex),
    ] {
        let pointer = GuestAddress(read(&memory, out));
        assert_ne!(pointer.0, 0);
        assert!(runtime.object(pointer).is_ok());
        runtime
            .release(&mut memory, &mut audio, None, pointer, interface)
            .unwrap();
    }
    assert_eq!(runtime.records.iter().flatten().count(), 1);
}
