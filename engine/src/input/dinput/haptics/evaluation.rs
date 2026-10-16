use super::device::Playback32;
use super::{Condition32, EffectDevice32, EffectKind32 as K, EffectParameters32, EffectPayload32};

impl EffectDevice32 {
    pub fn evaluate(
        &mut self,
        now: u64,
        axes: &[i32; 32],
        buttons: &[u8; 16],
        exclusive: bool,
    ) -> [i32; 8] {
        if !exclusive {
            self.lose_acquisition();
            return [0; 8];
        }
        if self.paused.is_some() {
            return [0; 8];
        }
        let mut sum = [0.0f64; 8];
        for effect in &mut self.effects {
            if let Some(parameters) = &effect.downloaded
                && let Some(button) = parameters.trigger
            {
                let down = buttons[usize::from(button) / 8] & (1 << (button % 8)) != 0;
                if effect.armed
                    && down
                    && (!effect.previous_trigger
                        || (effect.elapsed(now).is_none()
                            && parameters.trigger_repeat != u32::MAX
                            && effect.last_trigger.is_some_and(|last| {
                                now.saturating_sub(last) / 1000
                                    >= u64::from(parameters.duration)
                                        + u64::from(parameters.trigger_repeat)
                            })))
                {
                    effect.playing = Some(Playback32 {
                        start: now,
                        iterations: 1,
                        triggered: true,
                    });
                    effect.last_trigger = Some(now);
                }
                if !down && effect.playing.is_some_and(|playback| playback.triggered) {
                    effect.playing = None;
                }
                effect.previous_trigger = down;
            }
            if !self.actuators {
                continue;
            }
            let Some(mut elapsed) = effect.elapsed(now) else {
                continue;
            };
            let parameters = effect.downloaded.as_ref().unwrap();
            if parameters.sample_period != 0 {
                elapsed = elapsed / u64::from(parameters.sample_period)
                    * u64::from(parameters.sample_period);
            }
            let vector = parameters
                .direction_vector()
                .expect("downloaded direction was validated");
            let gain = f64::from(parameters.gain) * f64::from(self.gain) / 100_000_000.;
            for (axis_index, direction) in vector
                .iter()
                .enumerate()
                .take(parameters.axis_count as usize)
            {
                let axis = usize::from(parameters.axes[axis_index]);
                let value = parameters.value(effect.kind, elapsed, axis_index, axes, &vector);
                let directed = if matches!(&parameters.payload,EffectPayload32::Conditions{count,..} if *count>1)
                    || matches!(&parameters.payload,EffectPayload32::Custom{channels,..} if *channels>1)
                {
                    value
                } else {
                    value * direction
                };
                sum[axis] += directed * gain;
            }
        }
        if !self.actuators {
            return [0; 8];
        }
        if self.autocenter {
            for (index, value) in sum
                .iter_mut()
                .enumerate()
                .take(self.capabilities.axes as usize)
            {
                *value -= f64::from(axes[index]) * 10000. / 32768. * 0.1;
            }
        }
        sum.map(|value| value.round().clamp(-10000., 10000.) as i32)
    }
}

impl EffectParameters32 {
    fn value(
        &self,
        kind: K,
        elapsed: u64,
        axis_index: usize,
        axes: &[i32; 32],
        vector: &[f64; 8],
    ) -> f64 {
        let seconds = elapsed as f64;
        let value = match &self.payload {
            EffectPayload32::Constant(value) => f64::from(*value),
            EffectPayload32::Ramp(first, last) => {
                let ratio = if self.duration == u32::MAX {
                    0.
                } else {
                    (seconds / f64::from(self.duration.max(1))).min(1.)
                };
                f64::from(*first) + (f64::from(*last) - f64::from(*first)) * ratio
            }
            EffectPayload32::Periodic {
                magnitude,
                offset,
                phase,
                period,
            } => {
                let phase = ((elapsed % u64::from(*period)) as f64 / f64::from(*period)
                    + f64::from(*phase) / 36000.)
                    .fract();
                let wave = match kind {
                    K::Square => {
                        if phase < 0.5 {
                            1.
                        } else {
                            -1.
                        }
                    }
                    K::Sine => (phase * core::f64::consts::TAU).sin(),
                    K::Triangle => 1. - 4. * (phase - 0.25).abs().min((phase - 1.25).abs()),
                    K::SawtoothUp => 2. * phase - 1.,
                    K::SawtoothDown => 1. - 2. * phase,
                    _ => 0.,
                };
                return self.envelope_value(f64::from(*magnitude), elapsed) * wave
                    + f64::from(*offset);
            }
            EffectPayload32::Conditions { count, values } => {
                let condition = values[if *count == 1 { 0 } else { axis_index }];
                let aspect = match kind {
                    K::Damper | K::Friction => 8,
                    K::Inertia => 16,
                    _ => 0,
                };
                let axis = usize::from(self.axes[axis_index]);
                let source = if *count == 1 {
                    (0..self.axis_count as usize)
                        .map(|index| {
                            f64::from(axes[aspect + usize::from(self.axes[index])]) * vector[index]
                        })
                        .sum::<f64>()
                        * 10000.
                        / 32768.
                } else {
                    f64::from(axes[aspect + axis]) * 10000. / 32768.
                };
                return condition_force(condition, source, kind == K::Friction);
            }
            EffectPayload32::Custom {
                channels,
                period,
                samples,
            } => {
                if *channels > 1 && axis_index >= usize::from(*channels) {
                    return 0.0;
                }
                let frames = samples.len() / usize::from(*channels);
                let index = ((elapsed / u64::from(*period)) % frames as u64) as usize;
                f64::from(
                    samples[index * usize::from(*channels) + axis_index % usize::from(*channels)],
                )
            }
        };
        self.envelope_value(value, elapsed)
    }
    fn envelope_value(&self, value: f64, elapsed: u64) -> f64 {
        let Some(envelope) = self.envelope else {
            return value;
        };
        let magnitude = value.abs();
        let mut level = magnitude;
        if elapsed < u64::from(envelope.attack_time) {
            let ratio = elapsed as f64 / f64::from(envelope.attack_time);
            level = f64::from(envelope.attack_level)
                + (magnitude - f64::from(envelope.attack_level)) * ratio;
        }
        if self.duration != u32::MAX && envelope.fade_time != 0 {
            let remaining = u64::from(self.duration).saturating_sub(elapsed);
            if remaining < u64::from(envelope.fade_time) {
                let ratio = remaining as f64 / f64::from(envelope.fade_time);
                level = level.min(
                    f64::from(envelope.fade_level)
                        + (magnitude - f64::from(envelope.fade_level)) * ratio,
                );
            }
        }
        level * value.signum()
    }
}

fn condition_force(condition: Condition32, source: f64, friction: bool) -> f64 {
    let displacement = source - f64::from(condition.offset);
    let outside =
        (displacement.abs() - f64::from(condition.deadband)).max(0.) * displacement.signum();
    if outside == 0. {
        return 0.;
    }
    let coefficient = if outside >= 0. {
        condition.positive_coefficient
    } else {
        condition.negative_coefficient
    };
    let force = if friction {
        -outside.signum() * f64::from(coefficient)
    } else {
        -outside * f64::from(coefficient) / 10000.
    };
    force.clamp(
        -f64::from(condition.negative_saturation),
        f64::from(condition.positive_saturation),
    )
}
