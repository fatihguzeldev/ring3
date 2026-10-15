use super::*;
#[test]
fn inverse_distance_pan_cone_doppler_and_disabled_3d_have_observable_output_controls() {
    let listener = Listener3D::default();
    let mut source = Buffer3D::default();
    source.position = Vector3 {
        x: 2.0,
        y: 0.0,
        z: 0.0,
    };
    let controls = source.controls(listener, 48_000, 0, false);
    assert_eq!(controls.gains, [0.0, 0.5]);
    assert_eq!(controls.frequency, 48_000);
    source.velocity = Vector3 {
        x: 10.0,
        y: 0.0,
        z: 0.0,
    };
    assert!(source.controls(listener, 48_000, 0, false).frequency < 48_000);
    source.max_distance = 2.0;
    assert_eq!(source.controls(listener, 48_000, 0, true).gains, [0.0; 2]);
    source.mode = 2;
    assert_eq!(source.controls(listener, 48_000, 0, true).gains, [1.0; 2]);
    assert!(
        Listener3D {
            distance_factor: f32::NAN,
            ..listener
        }
        .validate()
        .is_err()
    );
}
#[test]
fn deferred_listener_and_buffer_parameters_change_applied_state_only_on_commit() {
    let mut device = Device::new(true, 48_000);
    let value = Listener3D {
        position: Vector3 {
            x: 3.0,
            y: 0.0,
            z: 0.0,
        },
        ..Listener3D::default()
    };
    device.set_listener(value, 1).unwrap();
    assert_eq!(device.listener, value);
    assert_eq!(device.applied_listener, Listener3D::default());
    device.applied_listener = device.listener;
    assert_eq!(device.applied_listener, value);
}

#[test]
fn extreme_finite_geometry_and_velocities_never_create_nonfinite_mixer_controls() {
    let listener = Listener3D {
        position: Vector3 {
            x: -f32::MAX,
            y: 0.0,
            z: 0.0,
        },
        velocity: Vector3 {
            x: f32::MAX,
            y: 0.0,
            z: 0.0,
        },
        distance_factor: f32::MAX,
        ..Listener3D::default()
    };
    let source = Buffer3D {
        position: Vector3 {
            x: f32::MAX,
            y: 0.0,
            z: 0.0,
        },
        velocity: Vector3 {
            x: f32::MAX,
            y: 0.0,
            z: 0.0,
        },
        ..Buffer3D::default()
    };
    assert!(listener.validate().is_ok());
    assert!(source.validate().is_ok());
    let controls = source.controls(listener, 48_000, 0, false);
    assert!(controls.gains.iter().all(|gain| gain.is_finite()));
    assert!((100..=200_000).contains(&controls.frequency));
}
