use crate::input::dinput::DirectInputResult32 as H;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HapticCapabilities32 {
    pub axes: u8,
    pub max_effects: u16,
}
impl HapticCapabilities32 {
    pub fn validate(self) -> Result<(), H> {
        if !(1..=8).contains(&self.axes) || !(1..=256).contains(&self.max_effects) {
            Err(H::INVALID_PARAM)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HapticFrame32 {
    pub epoch: u64,
    pub sequence: u64,
    pub slot: u8,
    pub timestamp_ns: u64,
    pub forces: [i32; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EffectKind32 {
    Constant = 0,
    Ramp = 1,
    Square = 2,
    Sine = 3,
    Triangle = 4,
    SawtoothUp = 5,
    SawtoothDown = 6,
    Spring = 7,
    Damper = 8,
    Inertia = 9,
    Friction = 10,
    Custom = 11,
}
impl EffectKind32 {
    pub const ALL: [Self; 12] = [
        Self::Constant,
        Self::Ramp,
        Self::Square,
        Self::Sine,
        Self::Triangle,
        Self::SawtoothUp,
        Self::SawtoothDown,
        Self::Spring,
        Self::Damper,
        Self::Inertia,
        Self::Friction,
        Self::Custom,
    ];
    pub fn guid(self) -> [u8; 16] {
        let a = (0x13541c20u32 + self as u32).to_le_bytes();
        [
            a[0], a[1], a[2], a[3], 0x33, 0x8e, 0xd0, 0x11, 0x9a, 0xd0, 0, 0xa0, 0xc9, 0xa0, 0x6e,
            0x35,
        ]
    }
    pub fn from_guid(guid: [u8; 16]) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.guid() == guid)
    }
    pub fn family(self) -> u32 {
        match self {
            Self::Constant => 1,
            Self::Ramp => 2,
            Self::Square | Self::Sine | Self::Triangle | Self::SawtoothUp | Self::SawtoothDown => 3,
            Self::Spring | Self::Damper | Self::Inertia | Self::Friction => 4,
            Self::Custom => 5,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Constant => "Constant force",
            Self::Ramp => "Ramp force",
            Self::Square => "Square",
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::SawtoothUp => "Sawtooth up",
            Self::SawtoothDown => "Sawtooth down",
            Self::Spring => "Spring",
            Self::Damper => "Damper",
            Self::Inertia => "Inertia",
            Self::Friction => "Friction",
            Self::Custom => "Custom force",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coordinates32 {
    Cartesian,
    Polar,
    Spherical,
}
impl Coordinates32 {
    pub fn flag(self) -> u32 {
        match self {
            Self::Cartesian => 0x10,
            Self::Polar => 0x20,
            Self::Spherical => 0x40,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectEnvelope32 {
    pub attack_level: u32,
    pub attack_time: u32,
    pub fade_level: u32,
    pub fade_time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Condition32 {
    pub offset: i32,
    pub positive_coefficient: i32,
    pub negative_coefficient: i32,
    pub positive_saturation: u32,
    pub negative_saturation: u32,
    pub deadband: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectPayload32 {
    Constant(i32),
    Ramp(i32, i32),
    Periodic {
        magnitude: u32,
        offset: i32,
        phase: u32,
        period: u32,
    },
    Conditions {
        count: u8,
        values: [Condition32; 8],
    },
    Custom {
        channels: u8,
        period: u32,
        samples: Arc<[i32]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectParameters32 {
    pub duration: u32,
    pub sample_period: u32,
    pub gain: u32,
    pub trigger: Option<u8>,
    pub trigger_repeat: u32,
    pub axis_count: u8,
    pub axes: [u8; 8],
    pub coordinates: Coordinates32,
    pub direction: [i32; 8],
    pub envelope: Option<EffectEnvelope32>,
    pub payload: EffectPayload32,
    pub start_delay: u32,
}
impl EffectParameters32 {
    pub fn empty(kind: EffectKind32) -> Self {
        let payload = match kind.family() {
            1 => EffectPayload32::Constant(0),
            2 => EffectPayload32::Ramp(0, 0),
            3 => EffectPayload32::Periodic {
                magnitude: 0,
                offset: 0,
                phase: 0,
                period: 1,
            },
            4 => EffectPayload32::Conditions {
                count: 1,
                values: [Condition32::default(); 8],
            },
            _ => EffectPayload32::Custom {
                channels: 1,
                period: 1,
                samples: Arc::from([]),
            },
        };
        Self {
            duration: u32::MAX,
            sample_period: 0,
            gain: 10000,
            trigger: None,
            trigger_repeat: u32::MAX,
            axis_count: 0,
            axes: [0; 8],
            coordinates: Coordinates32::Cartesian,
            direction: [0; 8],
            envelope: None,
            payload,
            start_delay: 0,
        }
    }
    pub fn merge(&self, next: &Self, mask: u32) -> Self {
        let mut result = self.clone();
        if mask & 1 != 0 {
            result.duration = next.duration;
        }
        if mask & 2 != 0 {
            result.sample_period = next.sample_period;
        }
        if mask & 4 != 0 {
            result.gain = next.gain;
        }
        if mask & 8 != 0 {
            result.trigger = next.trigger;
        }
        if mask & 16 != 0 {
            result.trigger_repeat = next.trigger_repeat;
        }
        if mask & 32 != 0 {
            result.axis_count = next.axis_count;
            result.axes = next.axes;
        }
        if mask & 64 != 0 {
            result.coordinates = next.coordinates;
            result.direction = next.direction;
        }
        if mask & 128 != 0 {
            result.envelope = next.envelope;
        }
        if mask & 256 != 0 {
            result.payload = next.payload.clone();
        }
        if mask & 512 != 0 {
            result.start_delay = next.start_delay;
        }
        result
    }
    pub fn validate(
        &self,
        kind: EffectKind32,
        capabilities: HapticCapabilities32,
        initialized: u32,
    ) -> Result<(), H> {
        capabilities.validate()?;
        let signed = |value: i32| (-10000..=10000).contains(&value);
        if self.gain > 10000
            || self.trigger.is_some_and(|button| button >= 128)
            || self.axis_count > capabilities.axes
            || (initialized & 32 != 0 && self.axis_count == 0)
        {
            return Err(H::INVALID_PARAM);
        }
        if initialized & 32 != 0 {
            let mut used = 0u16;
            for axis in &self.axes[..self.axis_count as usize] {
                if *axis >= capabilities.axes || used & (1 << axis) != 0 {
                    return Err(H::INVALID_PARAM);
                }
                used |= 1 << axis;
            }
        }
        if initialized & 64 != 0 {
            self.direction_vector()?;
        }
        if let Some(envelope) = self.envelope
            && (envelope.attack_level > 10000 || envelope.fade_level > 10000)
        {
            return Err(H::INVALID_PARAM);
        }
        if initialized & 256 == 0 {
            return Ok(());
        }
        let valid = match (&self.payload, kind.family()) {
            (EffectPayload32::Constant(value), 1) => signed(*value),
            (EffectPayload32::Ramp(first, last), 2) => signed(*first) && signed(*last),
            (
                EffectPayload32::Periodic {
                    magnitude,
                    offset,
                    phase,
                    period,
                },
                3,
            ) => *magnitude <= 10000 && signed(*offset) && *phase < 36000 && *period != 0,
            (EffectPayload32::Conditions { count, values }, 4) => {
                (*count == 1 || *count == self.axis_count)
                    && *count != 0
                    && *count <= 8
                    && values[..*count as usize].iter().all(|value| {
                        signed(value.offset)
                            && signed(value.positive_coefficient)
                            && signed(value.negative_coefficient)
                            && value.positive_saturation <= 10000
                            && value.negative_saturation <= 10000
                            && (0..=10000).contains(&value.deadband)
                    })
            }
            (
                EffectPayload32::Custom {
                    channels,
                    period,
                    samples,
                },
                5,
            ) => {
                *channels != 0
                    && *channels <= self.axis_count
                    && *period != 0
                    && !samples.is_empty()
                    && samples.len() <= 32768
                    && samples.len().is_multiple_of(*channels as usize)
                    && samples.iter().all(|value| signed(*value))
            }
            _ => false,
        };
        if valid { Ok(()) } else { Err(H::INVALID_PARAM) }
    }
}
