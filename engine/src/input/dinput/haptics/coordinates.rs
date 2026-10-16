use super::{Coordinates32, EffectParameters32};
use crate::input::dinput::DirectInputResult32 as H;

impl EffectParameters32 {
    pub fn direction_vector(&self) -> Result<[f64; 8], H> {
        let count = self.axis_count as usize;
        if count == 0 || count > 8 {
            return Err(H::INVALID_PARAM);
        }
        let mut vector = [0.; 8];
        match self.coordinates {
            Coordinates32::Cartesian => {
                let length = self.direction[..count]
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if length != 0. {
                    for (to, from) in vector[..count].iter_mut().zip(&self.direction[..count]) {
                        *to = f64::from(*from) / length;
                    }
                }
            }
            Coordinates32::Polar => {
                if count != 2 || !(0..36000).contains(&self.direction[0]) || self.direction[1] != 0
                {
                    return Err(H::INVALID_PARAM);
                }
                let angle = f64::from(self.direction[0]) * core::f64::consts::TAU / 36000.;
                vector[0] = angle.sin();
                vector[1] = -angle.cos();
            }
            Coordinates32::Spherical => {
                if count < 2
                    || self.direction[count - 1] != 0
                    || self.direction[..count - 1]
                        .iter()
                        .any(|value| !(0..36000).contains(value))
                {
                    return Err(H::INVALID_PARAM);
                }
                let first = f64::from(self.direction[0]) * core::f64::consts::TAU / 36000.;
                vector[0] = first.cos();
                vector[1] = first.sin();
                for axis in 2..count {
                    let angle =
                        f64::from(self.direction[axis - 1]) * core::f64::consts::TAU / 36000.;
                    for value in &mut vector[..axis] {
                        *value *= angle.cos();
                    }
                    vector[axis] = angle.sin();
                }
            }
        }
        Ok(vector)
    }
    pub fn converted_direction(&self, coordinate: Coordinates32) -> Result<[i32; 8], H> {
        if coordinate == self.coordinates {
            return Ok(self.direction);
        }
        let count = self.axis_count as usize;
        let vector = self.direction_vector()?;
        let mut result = [0; 8];
        let angle = |value: f64| {
            ((value * 36000. / core::f64::consts::TAU).round() as i32).rem_euclid(36000)
        };
        match coordinate {
            Coordinates32::Cartesian => {
                for (to, from) in result[..count].iter_mut().zip(&vector[..count]) {
                    *to = (*from * 10000.).round() as i32;
                }
            }
            Coordinates32::Polar => {
                if count != 2 {
                    return Err(H::INVALID_PARAM);
                }
                result[0] = angle(vector[0].atan2(-vector[1]));
            }
            Coordinates32::Spherical => {
                if count < 2 {
                    return Err(H::INVALID_PARAM);
                }
                result[0] = angle(vector[1].atan2(vector[0]));
                for axis in 2..count {
                    result[axis - 1] = angle(
                        vector[axis].atan2(
                            vector[..axis]
                                .iter()
                                .map(|value| value * value)
                                .sum::<f64>()
                                .sqrt(),
                        ),
                    );
                }
            }
        }
        Ok(result)
    }
}
