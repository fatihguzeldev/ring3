use super::support::*;
use crate::{
    input::{InputKind, TimedInputEvent, dinput::InterfaceKind32},
    process::{EngineInstance, HostError, MediaConfig},
};

fn timed_engine() -> (EngineInstance, crate::memory::GuestAddress, u64) {
    let (mut runtime, mut memory, pointer, _) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    let mut engine = EngineInstance::new(16, 77).unwrap();
    engine.memory = Some(memory);
    engine.dinput = Some(runtime);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    let epoch = engine.configure_media(MediaConfig::default()).unwrap();
    engine.observe_cpu_clock(100_000_000).unwrap();
    engine
        .submit_windows_input(
            epoch,
            1,
            &[TimedInputEvent {
                timestamp_ns: 1_000_000,
                kind: InputKind::Focus { focused: true },
            }],
        )
        .unwrap();
    let source = engine.media.as_ref().unwrap().input.source();
    engine
        .dinput
        .as_mut()
        .unwrap()
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(source, 0)
        .unwrap();
    (engine, pointer, epoch)
}

#[test]
fn delayed_windows_input_batch_keeps_twenty_milliseconds_in_directinput_data() {
    let (mut engine, pointer, epoch) = timed_engine();
    engine
        .submit_windows_input(
            epoch,
            2,
            &[
                TimedInputEvent {
                    timestamp_ns: 10_000_000,
                    kind: InputKind::Key {
                        code: 30,
                        pressed: true,
                    },
                },
                TimedInputEvent {
                    timestamp_ns: 30_000_000,
                    kind: InputKind::Key {
                        code: 30,
                        pressed: false,
                    },
                },
            ],
        )
        .unwrap();
    let EngineInstance { memory, dinput, .. } = &mut engine;
    let memory = memory.as_mut().unwrap();
    memory
        .write(crate::memory::GuestAddress(DATA + 512), &2u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        dinput
            .as_mut()
            .unwrap()
            .get_data(
                memory,
                pointer,
                &[pointer.0, 20, DATA + 768, DATA + 512, 0],
                InterfaceKind32::Device8A,
            )
            .unwrap(),
        0
    );
    let words = crate::process::windows::dinput::memory::words::<10>(memory, DATA + 768).unwrap();
    assert_eq!([words[0], words[1], words[2]], [3, 0x80, 10]);
    assert_eq!([words[5], words[6], words[7]], [3, 0, 30]);
    assert_eq!(words[7] - words[2], 20);
    assert!(words[8] > words[3]);
}

#[test]
fn windows_input_time_tracks_both_owners_policy_and_media_reset() {
    let (mut engine, _, epoch) = timed_engine();
    assert_eq!(
        engine.windows_input_time().unwrap(),
        (epoch, 1_000_000, 100_000_000)
    );
    engine
        .submit_windows_pointer(epoch, 50_000_000, 4, 5)
        .unwrap();
    assert_eq!(
        engine.windows_input_time().unwrap(),
        (epoch, 50_000_000, 100_000_000)
    );
    let prepared = engine.prepare_windows_input_flags(true, false).unwrap();
    engine.commit_windows_input_flags(prepared);
    assert_eq!(
        engine.windows_input_time().unwrap(),
        (epoch, 100_000_000, 100_000_000)
    );
    engine.observe_cpu_clock(120_000_000).unwrap();
    let prepared = engine.prepare_dinput_policy().unwrap();
    engine.commit_dinput_policy(prepared, (false, false));
    assert_eq!(
        engine.windows_input_time().unwrap(),
        (epoch, 120_000_000, 120_000_000)
    );
    let next = engine.reset_media().unwrap();
    assert_ne!(next, epoch);
    assert_eq!(
        engine.windows_input_time().unwrap(),
        (next, 120_000_000, 120_000_000)
    );
    engine.close();
    assert_eq!(engine.windows_input_time(), Err(HostError::Closed));
}

#[test]
fn native_policy_floor_rejects_old_physical_time_then_accepts_clamped_time() {
    let (mut engine, _, epoch) = timed_engine();
    let prepared = engine.prepare_dinput_policy().unwrap();
    engine.commit_dinput_policy(prepared, (false, false));
    let (_, floor, observed) = engine.windows_input_time().unwrap();
    assert_eq!((floor, observed), (100_000_000, 100_000_000));
    let old = TimedInputEvent {
        timestamp_ns: 30_000_000,
        kind: InputKind::Key {
            code: 30,
            pressed: true,
        },
    };
    let source_before = engine.input().unwrap().source().tail();
    assert!(engine.submit_windows_input(epoch, 2, &[old]).is_err());
    assert_eq!(engine.input().unwrap().source().tail(), source_before);
    assert_eq!(engine.input().unwrap().last_sequence(), 1);
    engine
        .submit_windows_input(
            epoch,
            2,
            &[TimedInputEvent {
                timestamp_ns: floor,
                ..old
            }],
        )
        .unwrap();
    assert!(engine.input().unwrap().state().keys[30]);
}
