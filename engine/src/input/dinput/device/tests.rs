use super::*;
use crate::input::{InputKind, TimedInputEvent, dinput::format::*, source::InputSource32};
fn source() -> InputSource32 {
    let mut source = InputSource32::new(1, 10_000_000, 512).unwrap();
    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 11_000_000,
                kind: InputKind::Focus { focused: true },
            }]
            .into_iter(),
        )
        .unwrap();
    source
}
fn keyboard(source: &InputSource32) -> DirectInputDevice32 {
    let mut device = DirectInputDevice32::new(DeviceDescription32::keyboard(), 16).unwrap();
    device
        .set_format(
            DataFormat32::new(
                DeviceDescription32::keyboard(),
                8,
                false,
                &[FormatObject32 {
                    offset: 3,
                    selector: ObjectSelector32::Key,
                    kind: ObjectType32::Button,
                    instance: Some(30),
                    aspect: 0,
                    optional: false,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    device.set_buffer_size(4).unwrap();
    device
        .set_cooperative(
            CooperativeLevel32::new(1, 6, crate::input::dinput::DeviceKind32::Keyboard).unwrap(),
        )
        .unwrap();
    device.acquire(source, 1).unwrap();
    device
}
#[test]
fn consumers_peek_and_immediate_reads_are_independent() {
    let mut source = source();
    let mut a = keyboard(&source);
    let mut b = keyboard(&source);
    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 12_000_000,
                kind: InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }]
            .into_iter(),
        )
        .unwrap();
    assert!(a.synchronize(&source, 1, 10_000_000));
    assert!(b.synchronize(&source, 1, 10_000_000));
    let mut state = [0; 8];
    a.read_state(&mut state).unwrap();
    a.commit_state_read();
    assert_eq!(state[3], 0x80);
    assert_eq!(a.buffered().unwrap(), 1);
    let mut events = [DeviceEvent32::default(); 4];
    assert_eq!(a.read_buffer(&mut events).unwrap().0, 1);
    assert_eq!(events[0].timestamp_ms, 2);
    a.consume_buffer(1).unwrap();
    assert_eq!(b.buffered().unwrap(), 1);
    assert_eq!(b.read_buffer(&mut events).unwrap().0, 1);
    assert_eq!(b.buffered().unwrap(), 1);
}
#[test]
fn buffer_size_reserves_sentinel_and_overflow_survives_peek() {
    let mut source = source();
    let mut device = keyboard(&source);
    for index in 0..4 {
        source
            .publish_physical(
                [TimedInputEvent {
                    timestamp_ns: 12_000_000 + index,
                    kind: InputKind::Key {
                        code: 30,
                        pressed: index % 2 == 0,
                    },
                }]
                .into_iter(),
            )
            .unwrap();
    }
    device.synchronize(&source, 1, 10_000_000);
    let mut events = [DeviceEvent32::default(); 8];
    assert_eq!(
        device.read_buffer(&mut events).unwrap(),
        (
            3,
            crate::input::dinput::DirectInputResult32::BUFFER_OVERFLOW
        )
    );
    assert_eq!(
        device.read_buffer(&mut events).unwrap().1,
        crate::input::dinput::DirectInputResult32::BUFFER_OVERFLOW
    );
    device.consume_buffer(3).unwrap();
    assert_eq!(
        device.read_buffer(&mut events).unwrap().1,
        crate::input::dinput::DirectInputResult32::OK
    );
}
#[test]
fn foreground_loss_is_latched_even_after_focus_returns() {
    let mut source = source();
    let mut device = keyboard(&source);
    source
        .publish_physical(
            [
                TimedInputEvent {
                    timestamp_ns: 12_000_000,
                    kind: InputKind::Focus { focused: false },
                },
                TimedInputEvent {
                    timestamp_ns: 13_000_000,
                    kind: InputKind::Focus { focused: true },
                },
            ]
            .into_iter(),
        )
        .unwrap();
    assert!(device.synchronize(&source, 1, 10_000_000));
    assert_eq!(
        device.check_acquired(),
        Err(crate::input::dinput::DirectInputResult32::INPUT_LOST)
    );
    device.acquire(&source, 1).unwrap();
    assert_eq!(device.buffered().unwrap(), 0);
}
#[test]
fn axis_mapping_covers_deadzone_saturation_and_signed_range() {
    let properties = AxisProperties32 {
        minimum: -100,
        maximum: 100,
        deadzone: 2000,
        saturation: 8000,
        raw: false,
    };
    assert_eq!(properties.map(0), 0);
    assert_eq!(properties.map(1000), 0);
    assert_eq!(properties.map(-32768), -100);
    assert_eq!(properties.map(32767), 100);
}

#[test]
fn controller_relative_reads_and_disconnect_reconnect_are_independent_transitions() {
    use crate::input::source::ControllerState32;
    let mut source = source();
    let connected = ControllerState32 {
        connected: true,
        ..Default::default()
    };
    source
        .publish_controller(1, 0, 12_000_000, connected)
        .unwrap();
    let description = DeviceDescription32 {
        kind: crate::input::dinput::DeviceKind32::Controller(0),
        axes: 1,
        povs: 0,
        buttons: 0,
    };
    let mut device = DirectInputDevice32::new(description, 16).unwrap();
    device
        .set_format(
            DataFormat32::new(
                description,
                4,
                true,
                &[FormatObject32 {
                    offset: 0,
                    selector: ObjectSelector32::Axis(0),
                    kind: ObjectType32::Axis,
                    instance: Some(0),
                    aspect: 0,
                    optional: false,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    device
        .set_cooperative(CooperativeLevel32::new(0, 10, description.kind).unwrap())
        .unwrap();
    device.acquire(&source, 0).unwrap();
    let mut moved = connected;
    moved.axes[0] = 32767;
    source.publish_controller(1, 0, 13_000_000, moved).unwrap();
    device.synchronize(&source, 0, 0);
    let mut state = [0; 4];
    device.read_state(&mut state).unwrap();
    assert_eq!(i32::from_le_bytes(state), 32767);
    device.commit_state_read();
    device.read_state(&mut state).unwrap();
    assert_eq!(state, [0; 4]);
    source
        .publish_controller(1, 0, 14_000_000, ControllerState32::default())
        .unwrap();
    source
        .publish_controller(1, 0, 15_000_000, connected)
        .unwrap();
    assert!(device.would_notify(&source, 0));
    assert!(device.synchronize(&source, 0, 0));
    assert_eq!(
        device.check_acquired(),
        Err(crate::input::dinput::DirectInputResult32::INPUT_LOST)
    );
}
