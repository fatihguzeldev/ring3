use super::HResult;
use crate::audio::AudioVoiceControls32;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl Vector3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    fn dot(self, other: Self) -> f32 {
        (f64::from(self.x) * f64::from(other.x)
            + f64::from(self.y) * f64::from(other.y)
            + f64::from(self.z) * f64::from(other.z))
        .clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
    }
    fn length(self) -> f32 {
        (f64::from(self.x).powi(2) + f64::from(self.y).powi(2) + f64::from(self.z).powi(2))
            .sqrt()
            .min(f64::from(f32::MAX)) as f32
    }
    fn normalized(self) -> Self {
        let length =
            (f64::from(self.x).powi(2) + f64::from(self.y).powi(2) + f64::from(self.z).powi(2))
                .sqrt();
        if length > 0.0 {
            Self {
                x: (f64::from(self.x) / length) as f32,
                y: (f64::from(self.y) / length) as f32,
                z: (f64::from(self.z) / length) as f32,
            }
        } else {
            Self::ZERO
        }
    }

    fn scale(self, value: f32) -> Self {
        Self {
            x: self.x * value,
            y: self.y * value,
            z: self.z * value,
        }
    }
    fn sub(self, other: Self) -> Self {
        Self {
            x: (f64::from(self.x) - f64::from(other.x))
                .clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
            y: (f64::from(self.y) - f64::from(other.y))
                .clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
            z: (f64::from(self.z) - f64::from(other.z))
                .clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
        }
    }
    fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener3D {
    pub position: Vector3,
    pub velocity: Vector3,
    pub front: Vector3,
    pub top: Vector3,
    pub distance_factor: f32,
    pub rolloff_factor: f32,
    pub doppler_factor: f32,
}
impl Default for Listener3D {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            front: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            top: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            distance_factor: 1.0,
            rolloff_factor: 1.0,
            doppler_factor: 1.0,
        }
    }
}
impl Listener3D {
    pub fn validate(self) -> Result<Self, HResult> {
        if !self.position.finite()
            || !self.velocity.finite()
            || !self.front.finite()
            || !self.top.finite()
            || self.front.length() == 0.0
            || self.top.length() == 0.0
            || self
                .front
                .normalized()
                .cross(self.top.normalized())
                .length()
                == 0.0
            || !self.distance_factor.is_finite()
            || self.distance_factor <= 0.0
            || !self.rolloff_factor.is_finite()
            || !(0.0..=10.0).contains(&self.rolloff_factor)
            || !self.doppler_factor.is_finite()
            || !(0.0..=10.0).contains(&self.doppler_factor)
        {
            Err(HResult::INVALID_PARAM)
        } else {
            Ok(self)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Buffer3D {
    pub position: Vector3,
    pub velocity: Vector3,
    pub inside_cone: u32,
    pub outside_cone: u32,
    pub cone_orientation: Vector3,
    pub outside_volume: i32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub mode: u32,
}
impl Default for Buffer3D {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            inside_cone: 360,
            outside_cone: 360,
            cone_orientation: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            outside_volume: 0,
            min_distance: 1.0,
            max_distance: 1_000_000_000.0,
            mode: 0,
        }
    }
}
impl Buffer3D {
    pub fn beyond_max_distance(self, listener: Listener3D) -> bool {
        if self.mode == 2 {
            return false;
        }
        let origin = if self.mode == 1 {
            Vector3::ZERO
        } else {
            listener.position
        };
        let x = f64::from(self.position.x) - f64::from(origin.x);
        let y = f64::from(self.position.y) - f64::from(origin.y);
        let z = f64::from(self.position.z) - f64::from(origin.z);
        x * x + y * y + z * z > f64::from(self.max_distance).powi(2)
    }

    pub fn validate(self) -> Result<Self, HResult> {
        if !self.position.finite()
            || !self.velocity.finite()
            || !self.cone_orientation.finite()
            || self.inside_cone > 360
            || self.outside_cone > 360
            || self.inside_cone > self.outside_cone
            || !(-10_000..=0).contains(&self.outside_volume)
            || !self.min_distance.is_finite()
            || self.min_distance <= 0.0
            || !self.max_distance.is_finite()
            || self.max_distance < self.min_distance
            || self.mode > 2
        {
            Err(HResult::INVALID_PARAM)
        } else {
            Ok(self)
        }
    }

    pub fn controls(
        self,
        listener: Listener3D,
        frequency: u32,
        volume: i32,
        mute_at_max: bool,
    ) -> AudioVoiceControls32 {
        if self.mode == 2 {
            return AudioVoiceControls32 {
                frequency,
                gains: [attenuation(volume); 2],
            };
        }
        let relative = if self.mode == 1 {
            self.position
        } else {
            self.position.sub(listener.position)
        };
        let distance = relative.length();
        let direction = relative.normalized();
        let right = if self.mode == 1 {
            Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }
        } else {
            listener
                .top
                .normalized()
                .cross(listener.front.normalized())
                .normalized()
        };
        let pan = direction.dot(right).clamp(-1.0, 1.0);
        let distance_gain = if mute_at_max && distance >= self.max_distance {
            0.0
        } else {
            let distance = distance.clamp(self.min_distance, self.max_distance);
            self.min_distance
                / (self.min_distance + listener.rolloff_factor * (distance - self.min_distance))
        };
        let angle = if self.cone_orientation.length() > 0.0 {
            self.cone_orientation
                .normalized()
                .dot(direction.scale(-1.0))
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees()
                * 2.0
        } else {
            0.0
        };
        let cone = if angle <= self.inside_cone as f32 {
            1.0
        } else if angle >= self.outside_cone as f32 {
            attenuation(self.outside_volume)
        } else {
            attenuation(
                (self.outside_volume as f32 * (angle - self.inside_cone as f32)
                    / (self.outside_cone - self.inside_cone) as f32) as i32,
            )
        };
        let gain = attenuation(volume) * distance_gain * cone;
        let speed = 343.5f64;
        let source = f64::from(self.velocity.dot(direction))
            * f64::from(listener.distance_factor)
            * f64::from(listener.doppler_factor);
        let receiver = if self.mode == 1 {
            0.0
        } else {
            f64::from(listener.velocity.dot(direction))
                * f64::from(listener.distance_factor)
                * f64::from(listener.doppler_factor)
        };
        let ratio = ((speed + receiver).max(speed * 0.1) / (speed + source).max(speed * 0.1))
            .clamp(0.1, 10.0);
        AudioVoiceControls32 {
            frequency: (f64::from(frequency) * ratio).clamp(100.0, 200_000.0) as u32,
            gains: [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))],
        }
    }
}

pub(super) fn attenuation(value: i32) -> f32 {
    if value <= -10_000 {
        0.0
    } else {
        10.0f32.powf(value as f32 / 2000.0)
    }
}
