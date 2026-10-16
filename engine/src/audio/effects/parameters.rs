use crate::audio::AudioError32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EffectKind {
    Chorus = 0,
    Compressor = 1,
    Distortion = 2,
    Echo = 3,
    Flanger = 4,
    Gargle = 5,
    ParametricEq = 6,
    I3dl2Reverb = 7,
    WavesReverb = 8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameters {
    pub(super) kind: EffectKind,
    pub(super) words: [u32; 12],
}

#[derive(Clone, Copy)]
enum Field {
    Float(f32, f32, f32),
    Integer(i32, i32, i32),
}
use Field::{Float as F, Integer as I};

impl EffectKind {
    pub const ALL: [Self; 9] = [
        Self::Chorus,
        Self::Compressor,
        Self::Distortion,
        Self::Echo,
        Self::Flanger,
        Self::Gargle,
        Self::ParametricEq,
        Self::I3dl2Reverb,
        Self::WavesReverb,
    ];

    fn fields(self) -> &'static [Field] {
        match self {
            Self::Chorus => &[
                F(0., 100., 50.),
                F(0., 100., 10.),
                F(-99., 99., 25.),
                F(0., 10., 1.1),
                I(0, 1, 1),
                F(0., 20., 16.),
                I(0, 4, 3),
            ],
            Self::Compressor => &[
                F(-60., 60., 0.),
                F(0.01, 500., 10.),
                F(50., 3000., 200.),
                F(-60., 0., -20.),
                F(1., 100., 3.),
                F(0., 4., 4.),
            ],
            Self::Distortion => &[
                F(-60., 0., -18.),
                F(0., 100., 15.),
                F(100., 8000., 2400.),
                F(100., 8000., 2400.),
                F(100., 8000., 8000.),
            ],
            Self::Echo => &[
                F(0., 100., 50.),
                F(0., 100., 50.),
                F(1., 2000., 500.),
                F(1., 2000., 500.),
                I(0, 1, 0),
            ],
            Self::Flanger => &[
                F(0., 100., 50.),
                F(0., 100., 100.),
                F(-99., 99., -50.),
                F(0., 10., 0.25),
                I(0, 1, 1),
                F(0., 4., 2.),
                I(0, 4, 2),
            ],
            Self::Gargle => &[I(1, 1000, 20), I(0, 1, 0)],
            Self::ParametricEq => &[F(80., 16000., 8000.), F(1., 36., 12.), F(-15., 15., 0.)],
            Self::I3dl2Reverb => &[
                I(-10000, 0, -1000),
                I(-10000, 0, -100),
                F(0., 10., 0.),
                F(0.1, 20., 1.49),
                F(0.1, 2., 0.83),
                I(-10000, 1000, -2602),
                F(0., 0.3, 0.007),
                I(-10000, 2000, 200),
                F(0., 0.1, 0.011),
                F(0., 100., 100.),
                F(0., 100., 100.),
                F(20., 20000., 5000.),
            ],
            Self::WavesReverb => &[
                F(-96., 0., 0.),
                F(-96., 0., 0.),
                F(0.001, 3000., 1000.),
                F(0.001, 0.999, 0.001),
            ],
        }
    }

    pub fn word_count(self) -> usize {
        self.fields().len()
    }

    pub fn defaults(self) -> Parameters {
        let mut words = [0; 12];
        for (word, field) in words.iter_mut().zip(self.fields()) {
            *word = match field {
                F(_, _, value) => value.to_bits(),
                I(_, _, value) => *value as u32,
            };
        }
        Parameters { kind: self, words }
    }
}

impl Parameters {
    pub fn parse(
        kind: EffectKind,
        words: &[u32],
        sample_rate: u32,
    ) -> Result<(Self, bool), AudioError32> {
        if words.len() != kind.word_count() || sample_rate == 0 {
            return Err(AudioError32::InvalidRequest);
        }
        for (word, field) in words.iter().zip(kind.fields()) {
            let valid = match field {
                F(min, max, _) => {
                    let value = f32::from_bits(*word);
                    value.is_finite() && (*min..=*max).contains(&value)
                }
                I(min, max, _) => (*min..=*max).contains(&(*word as i32)),
            };
            if !valid {
                return Err(AudioError32::InvalidRequest);
            }
        }
        let mut result = Self {
            kind,
            words: [0; 12],
        };
        result.words[..words.len()].copy_from_slice(words);
        let frequency_indices: &[usize] = match kind {
            EffectKind::Distortion => &[2, 3, 4],
            EffectKind::ParametricEq => &[0],
            _ => &[],
        };
        let mut clamped = false;
        for index in frequency_indices {
            let value = result.float(*index);
            let limit = sample_rate as f32 / 3.;
            if value > limit {
                result.words[*index] = limit.to_bits();
                clamped = true;
            }
        }
        Ok((result, clamped))
    }
    pub fn kind(self) -> EffectKind {
        self.kind
    }
    pub fn words(&self) -> &[u32] {
        &self.words[..self.kind.word_count()]
    }
    pub(super) fn float(self, index: usize) -> f32 {
        f32::from_bits(self.words[index])
    }
    pub(super) fn integer(self, index: usize) -> i32 {
        self.words[index] as i32
    }
}
