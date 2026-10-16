use super::{EffectKind as K, chain::Effect};

impl Effect {
    pub fn update_coefficients(&mut self) {
        let p = self.parameters;
        let rate = self.rate as f64;
        let float = |index| f64::from(p.float(index));
        self.coefficients = [0.; 16];
        match p.kind() {
            K::Chorus | K::Flanger => {
                self.coefficients[..5].copy_from_slice(&[
                    float(0) / 100.,
                    float(1) / 100.,
                    float(2) / 100.,
                    float(3) / rate,
                    float(5) * rate / 1000.,
                ]);
            }
            K::Echo => self.coefficients[..4].copy_from_slice(&[
                float(0) / 100.,
                float(1) / 100.,
                float(2) * rate / 1000.,
                float(3) * rate / 1000.,
            ]),
            K::Compressor => self.coefficients[..6].copy_from_slice(&[
                10.0f64.powf(float(0) / 20.),
                (-1000. / (rate * float(1))).exp(),
                (-1000. / (rate * float(2))).exp(),
                10.0f64.powf(float(3) / 20.),
                1. / float(4) - 1.,
                float(5) * rate / 1000.,
            ]),
            K::Distortion => {
                self.coefficients[..3].copy_from_slice(&[
                    10.0f64.powf(float(0) / 20.),
                    1. + float(1) * 0.2,
                    1. - (-core::f64::consts::TAU * float(4) / rate).exp(),
                ]);
                let semitones = (12. * (1. + float(3) / float(2)).log2()).clamp(1., 36.);
                self.filter
                    .peak(self.rate, float(2), semitones, float(1) * 0.15);
            }
            K::Gargle => self.coefficients[0] = p.integer(0) as f64 / rate,
            K::ParametricEq => self.filter.peak(self.rate, float(0), float(1), float(2)),
            K::I3dl2Reverb | K::WavesReverb => {
                let (decay, hf_ratio, density, diffusion, reference) = if p.kind() == K::I3dl2Reverb
                {
                    (
                        float(3),
                        float(4),
                        float(10) / 100.,
                        float(9) / 100.,
                        float(11),
                    )
                } else {
                    (float(2) / 1000., float(3), 1., 1., 5000.)
                };
                for (index, milliseconds) in [29.7, 37.1, 41.1, 43.7, 47.9, 53.3, 59.3, 61.7]
                    .into_iter()
                    .enumerate()
                {
                    self.coefficients[index] =
                        milliseconds * rate / 1000. * (0.25 + density * 0.75);
                }
                self.coefficients[8] = 10.0f64.powf(-3. * self.coefficients[0] / (rate * decay));
                self.feedback_hf =
                    10.0f64.powf(-3. * self.coefficients[0] / (rate * decay * hf_ratio));
                self.coefficients[9] =
                    1. - (-core::f64::consts::TAU * reference.min(rate * 0.49) / rate).exp();
                self.coefficients[10] = diffusion;
                if p.kind() == K::I3dl2Reverb {
                    self.envelope = 10.0f64.powf(p.integer(1) as f64 / 2000.);
                    self.coefficients[11] =
                        10.0f64.powf(p.integer(0) as f64 / 2000.) / (1. + float(2));
                    self.coefficients[12] = 10.0f64.powf(p.integer(5) as f64 / 2000.);
                    self.coefficients[13] = float(6) * rate;
                    self.coefficients[14] = 10.0f64.powf(p.integer(7) as f64 / 2000.);
                    self.coefficients[15] = float(8) * rate;
                } else {
                    self.coefficients[11] = 10.0f64.powf(float(0) / 20.);
                    self.coefficients[14] = 10.0f64.powf(float(1) / 20.);
                }
            }
        }
    }

    pub fn process(&mut self, input: [f64; 2]) -> [f64; 2] {
        let p = self.parameters;
        let c = self.coefficients;
        match p.kind() {
            K::Chorus | K::Flanger => {
                let delayed: [f64; 2] = core::array::from_fn(|channel| {
                    let phase = (self.phase + channel as f64 * (p.integer(6) - 2) as f64 * 0.25)
                        .rem_euclid(1.);
                    let lfo = if p.integer(4) == 1 {
                        (phase * core::f64::consts::TAU).sin()
                    } else {
                        1. - 4. * (phase - 0.5).abs()
                    };
                    self.delay.read(c[4] * (1. + c[1] * lfo), channel)
                });
                self.phase = (self.phase + c[3]).fract();
                self.delay.write(core::array::from_fn(|channel| {
                    input[channel] + delayed[channel] * c[2]
                }));
                core::array::from_fn(|channel| {
                    input[channel] * (1. - c[0]) + delayed[channel] * c[0]
                })
            }
            K::Echo => {
                let delayed: [f64; 2] =
                    core::array::from_fn(|channel| self.delay.read(c[2 + channel], channel));
                self.delay.write(core::array::from_fn(|channel| {
                    input[channel]
                        + delayed[if p.integer(4) != 0 {
                            1 - channel
                        } else {
                            channel
                        }] * c[1]
                }));
                core::array::from_fn(|channel| {
                    input[channel] * (1. - c[0]) + delayed[channel] * c[0]
                })
            }
            K::Compressor => {
                let amplitude = input[0].abs().max(input[1].abs());
                let smoothing = if amplitude > self.envelope {
                    c[1]
                } else {
                    c[2]
                };
                self.envelope = amplitude + (self.envelope - amplitude) * smoothing;
                let gain = if self.envelope > c[3] {
                    (self.envelope / c[3]).powf(c[4])
                } else {
                    1.
                };
                let delayed: [f64; 2] = core::array::from_fn(|channel| {
                    if c[5] < 1. {
                        input[channel]
                    } else {
                        self.delay.read(c[5], channel)
                    }
                });
                self.delay.write(input);
                delayed.map(|value| value * gain * c[0])
            }
            K::Distortion => {
                for (channel, value) in input.into_iter().enumerate() {
                    self.lowpass[channel] += c[2] * (value - self.lowpass[channel]);
                }
                let shaped = self
                    .lowpass
                    .map(|value| (value * c[1]).tanh() / c[1].tanh() * c[0]);
                self.filter.process(shaped)
            }
            K::Gargle => {
                let gain = if p.integer(1) == 0 {
                    1. - 2. * (self.phase - 0.5).abs()
                } else {
                    u8::from(self.phase < 0.5) as f64
                };
                self.phase = (self.phase + c[0]).fract();
                input.map(|value| value * gain)
            }
            K::ParametricEq => self.filter.process(input),
            K::I3dl2Reverb | K::WavesReverb => self.reverberate(input),
        }
    }

    fn reverberate(&mut self, input: [f64; 2]) -> [f64; 2] {
        let p = self.parameters;
        let c = self.coefficients;
        let taps = match self.quality {
            0 => 2,
            1 => 4,
            2 => 6,
            _ => 8,
        };
        let wet: [f64; 2] = core::array::from_fn(|channel| {
            (0..taps)
                .map(|index| self.delay.read(c[index] + c[15], (channel + index) % 2))
                .sum::<f64>()
                / taps as f64
        });
        let hf_gain = if p.kind() == EffectKind::I3dl2Reverb {
            self.envelope
        } else {
            1.
        };
        let filtered: [f64; 2] = core::array::from_fn(|channel| {
            self.lowpass[channel] += c[9] * (wet[channel] - self.lowpass[channel]);
            self.lowpass[channel] + (wet[channel] - self.lowpass[channel]) * hf_gain
        });
        self.delay.write(core::array::from_fn(|channel| {
            let feedback = |channel| {
                self.lowpass[channel] * c[8]
                    + (wet[channel] - self.lowpass[channel]) * self.feedback_hf
            };
            let scattered =
                feedback(channel) * (1. - c[10] * 0.5) + feedback(1 - channel) * c[10] * 0.5;
            input[channel] + scattered
        }));
        core::array::from_fn(|channel| {
            let reflection = self.delay.read(c[13], channel) * c[12];
            if p.kind() == K::I3dl2Reverb {
                input[channel] + (reflection + filtered[channel] * c[14]) * c[11]
            } else {
                (input[channel] + filtered[channel] * c[14]) * c[11]
            }
        })
    }
}

use super::EffectKind;
