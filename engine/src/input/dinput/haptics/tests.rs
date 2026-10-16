use super::*;
use crate::input::dinput::DirectInputResult32 as H;

fn params(kind: EffectKind32) -> EffectParameters32 {
    let mut result = EffectParameters32::empty(kind);
    result.axis_count = 2;
    result.axes[1] = 1;
    result.direction[0] = 1;
    result.duration = 10000;
    result
}
fn device() -> EffectDevice32 {
    let mut device = EffectDevice32::new(HapticCapabilities32 {
        axes: 2,
        max_effects: 2,
    })
    .unwrap();
    device.set_autocenter(false);
    device
}

#[test]
fn constant_gain_envelope_pause_and_acquisition_are_real_force_state() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(8000);
    parameters.envelope = Some(EffectEnvelope32 {
        attack_level: 0,
        attack_time: 2000,
        fade_level: 0,
        fade_time: 2000,
    });
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    assert_eq!(
        device.evaluate(1_000_000, &[0; 32], &[0; 16], true)[0],
        4000
    );
    device.command(4, true, 1_000_000).unwrap();
    assert_eq!(device.evaluate(5_000_000, &[0; 32], &[0; 16], true), [0; 8]);
    device.command(8, true, 5_000_000).unwrap();
    assert_eq!(
        device.evaluate(5_000_000, &[0; 32], &[0; 16], true)[0],
        4000
    );
    assert_eq!(
        device.evaluate(6_000_000, &[0; 32], &[0; 16], false),
        [0; 8]
    );
    assert_eq!(
        device.start(id, 1, 0x80000000, true, 6_000_000),
        Err(H::NOT_DOWNLOADED)
    );
}

#[test]
fn undownloaded_updates_do_not_change_playing_force_until_download() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(2000);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters.clone()), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    parameters.payload = EffectPayload32::Constant(6000);
    device
        .set_parameters(id, parameters, 0x80000100, true, 1000)
        .unwrap();
    assert_eq!(device.evaluate(1000, &[0; 32], &[0; 16], true)[0], 2000);
    assert_eq!(device.download(id, true).unwrap(), 1);
    assert_eq!(device.evaluate(1000, &[0; 32], &[0; 16], true)[0], 6000);
    assert_eq!(
        device.set_parameters(
            id,
            device.parameters(id).unwrap().clone(),
            0xa0000100,
            true,
            1000
        ),
        Err(H::INVALID_PARAM)
    );
    device.unload(id).unwrap();
    assert_eq!(device.effect_status(id, 1000).unwrap(), 0);
}

#[test]
fn periodic_condition_custom_and_coordinate_transforms_are_distinguishable() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Sine);
    parameters.payload = EffectPayload32::Periodic {
        magnitude: 10000,
        offset: 0,
        phase: 0,
        period: 1000,
    };
    let prepared = device
        .prepare_effect(EffectKind32::Sine, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    assert_eq!(device.evaluate(250_000, &[0; 32], &[0; 16], true)[0], 10000);
    device.delete(id).unwrap();
    let mut parameters = params(EffectKind32::Spring);
    parameters.payload = EffectPayload32::Conditions {
        count: 1,
        values: [Condition32 {
            offset: 0,
            positive_coefficient: 10000,
            negative_coefficient: 10000,
            positive_saturation: 5000,
            negative_saturation: 5000,
            deadband: 1000,
        }; 8],
    };
    let prepared = device
        .prepare_effect(EffectKind32::Spring, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    let mut axes = [0; 32];
    axes[0] = 32767;
    assert_eq!(device.evaluate(1, &axes, &[0; 16], true)[0], -5000);
    device.delete(id).unwrap();
    let mut parameters = params(EffectKind32::Custom);
    parameters.payload = EffectPayload32::Custom {
        channels: 2,
        period: 10,
        samples: std::sync::Arc::from([1000, -2000, 3000, -4000]),
    };
    let prepared = device
        .prepare_effect(EffectKind32::Custom, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    assert_eq!(
        &device.evaluate(10_000, &[0; 32], &[0; 16], true)[..2],
        &[3000, -4000]
    );
    let mut direction = params(EffectKind32::Constant);
    direction.coordinates = Coordinates32::Polar;
    direction.direction = [9000, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        &direction
            .converted_direction(Coordinates32::Cartesian)
            .unwrap()[..2],
        &[10000, 0]
    );
}

#[test]
fn preparation_failure_and_rejected_axis_changes_leave_effect_ownership_unchanged() {
    let mut device = device();
    let prepared = device
        .prepare_effect(EffectKind32::Constant, None, false)
        .unwrap();
    let id = prepared.id();
    assert_eq!(device.len(), 0);
    drop(prepared);
    assert_eq!(device.len(), 0);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, None, false)
        .unwrap();
    assert_eq!(prepared.id(), id);
    device.commit_effect(prepared);
    assert_eq!(device.download(id, true), Err(H::INCOMPLETE_EFFECT));
    let parameters = params(EffectKind32::Constant);
    device
        .set_parameters(id, parameters.clone(), 0x800003ff, false, 0)
        .unwrap();
    let mut invalid = parameters.clone();
    invalid.axes[0] = 1;
    assert_eq!(
        device.set_parameters(id, invalid, 0x80000020, false, 0),
        Err(H::INVALID_PARAM)
    );
    assert_eq!(device.parameters(id).unwrap(), &parameters);
}

#[test]
fn starting_while_paused_uses_the_frozen_effect_clock() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(4000);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.command(4, true, 1_000_000).unwrap();
    device.start(id, 1, 0, true, 5_000_000).unwrap();
    device.command(8, true, 8_000_000).unwrap();
    assert_eq!(
        device.evaluate(8_000_000, &[0; 32], &[0; 16], true)[0],
        4000
    );
    assert_eq!(device.evaluate(18_000_000, &[0; 32], &[0; 16], true)[0], 0);
}

#[test]
fn actuator_muting_preserves_trigger_time_instead_of_restarting_on_unmute() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(4000);
    parameters.duration = 1000;
    parameters.trigger = Some(0);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    device.command(32, true, 0).unwrap();
    let mut buttons = [0; 16];
    buttons[0] = 1;
    assert_eq!(device.evaluate(0, &[0; 32], &buttons, true), [0; 8]);
    device.command(16, true, 2_000_000).unwrap();
    assert_eq!(device.evaluate(2_000_000, &[0; 32], &buttons, true), [0; 8]);
    assert_eq!(device.effect_status(id, 2_000_000).unwrap(), 0);
}

#[test]
fn downloaded_trigger_stops_on_release_without_stopping_a_manual_restart() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(4000);
    parameters.trigger = Some(0);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    let mut buttons = [0; 16];
    assert_eq!(device.evaluate(0, &[0; 32], &buttons, true)[0], 0);
    buttons[0] = 1;
    assert_eq!(device.evaluate(1000, &[0; 32], &buttons, true)[0], 4000);
    buttons[0] = 0;
    assert_eq!(device.evaluate(2000, &[0; 32], &buttons, true)[0], 0);
    assert_eq!(device.effect_status(id, 2000).unwrap(), 0);
    buttons[0] = 1;
    assert_eq!(device.evaluate(3000, &[0; 32], &buttons, true)[0], 4000);
    device.start(id, 1, 0, true, 4000).unwrap();
    buttons[0] = 0;
    assert_eq!(device.evaluate(5000, &[0; 32], &buttons, true)[0], 4000);
    assert_eq!(device.effect_status(id, 5000).unwrap(), 1);
    assert_eq!(device.evaluate(10_004_000, &[0; 32], &buttons, true)[0], 0);
    device.unload(id).unwrap();
    buttons[0] = 1;
    assert_eq!(device.evaluate(11_000_000, &[0; 32], &buttons, true)[0], 0);
    device.download(id, true).unwrap();
    buttons[0] = 0;
    device.evaluate(11_001_000, &[0; 32], &buttons, true);
    buttons[0] = 1;
    assert_eq!(
        device.evaluate(11_002_000, &[0; 32], &buttons, true)[0],
        4000
    );
}

#[test]
fn malformed_public_capabilities_and_trigger_cannot_reach_fixed_axis_or_button_storage() {
    let mut parameters = params(EffectKind32::Constant);
    parameters.axis_count = 9;
    assert_eq!(
        parameters.validate(
            EffectKind32::Constant,
            HapticCapabilities32 {
                axes: 9,
                max_effects: 2
            },
            ALL_PARAMETERS
        ),
        Err(H::INVALID_PARAM)
    );
    parameters.axis_count = 2;
    parameters.trigger = Some(128);
    assert_eq!(
        parameters.validate(
            EffectKind32::Constant,
            HapticCapabilities32 {
                axes: 2,
                max_effects: 2
            },
            ALL_PARAMETERS
        ),
        Err(H::INVALID_PARAM)
    );
    parameters.trigger = Some(127);
    assert!(
        parameters
            .validate(
                EffectKind32::Constant,
                HapticCapabilities32 {
                    axes: 2,
                    max_effects: 2
                },
                ALL_PARAMETERS
            )
            .is_ok()
    );
}
#[test]
fn start_parameter_updates_share_the_same_paused_clock_as_start_calls() {
    let mut device = device();
    let mut parameters = params(EffectKind32::Constant);
    parameters.payload = EffectPayload32::Constant(4000);
    let prepared = device
        .prepare_effect(EffectKind32::Constant, Some(parameters.clone()), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.command(4, true, 1_000_000).unwrap();
    device
        .set_parameters(id, parameters, 0x20000100, true, 5_000_000)
        .unwrap();
    device.command(8, true, 8_000_000).unwrap();
    assert_eq!(
        device.evaluate(8_000_000, &[0; 32], &[0; 16], true)[0],
        4000
    );
}

#[test]
fn multichannel_custom_force_leaves_extra_axes_silent_even_during_envelope_attack() {
    let mut device = EffectDevice32::new(HapticCapabilities32 {
        axes: 3,
        max_effects: 1,
    })
    .unwrap();
    device.set_autocenter(false);
    let mut parameters = params(EffectKind32::Custom);
    parameters.axis_count = 3;
    parameters.axes[2] = 2;
    parameters.payload = EffectPayload32::Custom {
        channels: 2,
        period: 1000,
        samples: std::sync::Arc::from([2000, -3000, 4000, -5000]),
    };
    parameters.envelope = Some(EffectEnvelope32 {
        attack_level: 1000,
        attack_time: 2000,
        fade_level: 0,
        fade_time: 0,
    });
    let prepared = device
        .prepare_effect(EffectKind32::Custom, Some(parameters), true)
        .unwrap();
    let id = prepared.id();
    device.commit_effect(prepared);
    device.start(id, 1, 0, true, 0).unwrap();
    assert_eq!(
        &device.evaluate(1_000_000, &[0; 32], &[0; 16], true)[..3],
        &[2500, -3000, 0]
    );
    assert_eq!(
        &device.evaluate(2_000_000, &[0; 32], &[0; 16], true)[..3],
        &[2000, -3000, 0]
    );
}
