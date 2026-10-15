use super::support::*;
use crate::{
    input::{InputKind, TimedInputEvent, dinput::device::DeviceEvent32},
    process::{EngineInstance, MediaConfig},
};
#[test]
fn buffered_timestamps_after_media_reset_keep_the_shared_cpu_tick_origin() {
    let (mut runtime, mut memory, pointer, _) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    let mut engine = EngineInstance::new(16, 77).unwrap();
    engine.memory = Some(memory);
    engine.dinput = Some(runtime);
    engine.configure_cpu_clock(1_000_000, 10_000_000).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    engine.observe_cpu_clock(20_000_000).unwrap();
    let epoch = engine.reset_media().unwrap();
    engine.observe_cpu_clock(22_000_000).unwrap();
    engine
        .submit_windows_input(
            epoch,
            1,
            &[TimedInputEvent {
                timestamp_ns: 21_000_000,
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
    engine
        .submit_windows_input(
            epoch,
            2,
            &[TimedInputEvent {
                timestamp_ns: 22_000_000,
                kind: InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }],
        )
        .unwrap();
    let mut events = [DeviceEvent32::default(); 4];
    let native = &engine
        .dinput
        .as_ref()
        .unwrap()
        .device(pointer)
        .unwrap()
        .native;
    assert_eq!(native.read_buffer(&mut events).unwrap().0, 1);
    let (origin, now) = engine.cpu_clock.monotonic_bounds().unwrap();
    assert_eq!(events[0].timestamp_ms, ((now - origin) / 1_000_000) as u32);
    assert_eq!(events[0].timestamp_ms, 12);
}
