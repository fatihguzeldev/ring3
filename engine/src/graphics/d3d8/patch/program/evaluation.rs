use super::{super::PatchError, SurfaceProgram};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceSample {
    pub value: [f32; 4],
    pub du: [f32; 4],
    pub dv: [f32; 4],
}

impl SurfaceSample {
    pub fn normal(self) -> [f32; 4] {
        let [x, y, z] = [
            self.du[1] * self.dv[2] - self.du[2] * self.dv[1],
            self.du[2] * self.dv[0] - self.du[0] * self.dv[2],
            self.du[0] * self.dv[1] - self.du[1] * self.dv[0],
        ];
        let length = (x * x + y * y + z * z).sqrt();
        if length > 0.0 {
            [x / length, y / length, z / length, 1.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }
}

fn power(value: f32, n: u32) -> f32 {
    (0..n).fold(1.0, |v, _| v * value)
}
fn derivative(value: f32, n: u32) -> f32 {
    if n == 0 {
        0.0
    } else {
        n as f32 * power(value, n - 1)
    }
}
fn polynomial(words: &[u32], t: f32, degree: usize) -> (f32, f32) {
    let mut value = f32::from_bits(words[degree]);
    let mut slope = 0.0;
    for i in (0..degree).rev() {
        slope = slope * t + value;
        value = value * t + f32::from_bits(words[i]);
    }
    (value, slope)
}
fn interval(t: f32, spans: u32) -> (u32, f32) {
    let scaled = t * spans as f32;
    let cell = (scaled.floor() as u32).min(spans - 1);
    (cell, scaled - cell as f32)
}

impl SurfaceProgram {
    pub fn evaluate(
        &self,
        uv: [f32; 2],
        mut control: impl FnMut(u32) -> [f32; 4],
    ) -> Result<SurfaceSample, PatchError> {
        if uv
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || (self.triangular() && uv[0] + uv[1] > 1.0000001)
        {
            return Err(PatchError);
        }
        let p = &self.words;
        let mut sample = SurfaceSample {
            value: [0.0; 4],
            du: [0.0; 4],
            dv: [0.0; 4],
        };
        let mut add = |index: u32, weight: f32, du: f32, dv: f32| {
            let point = control(index);
            for (lane, value) in point.into_iter().enumerate() {
                sample.value[lane] += weight * value;
                sample.du[lane] += du * value;
                sample.dv[lane] += dv * value;
            }
        };
        if self.triangular() {
            let w = 1.0 - uv[0] - uv[1];
            for index in 0..p[6] {
                let at = 8 + index as usize * 4;
                let (a, b, c, k) = (p[at], p[at + 1], p[at + 2], f32::from_bits(p[at + 3]));
                let (u, v, w0) = (power(uv[0], a), power(uv[1], b), power(w, c));
                add(
                    index,
                    k * u * v * w0,
                    k * v * (derivative(uv[0], a) * w0 - u * derivative(w, c)),
                    k * u * (derivative(uv[1], b) * w0 - v * derivative(w, c)),
                );
            }
        } else {
            let (x, u) = interval(uv[0], p[4]);
            let (y, v) = interval(uv[1], p[5]);
            for row in 0..=p[3] {
                let (b, db) = polynomial(&p[8 + row as usize * 6..], v, p[3] as usize);
                for col in 0..=p[3] {
                    let (a, da) = polynomial(&p[8 + col as usize * 6..], u, p[3] as usize);
                    add(
                        (y + row) * p[6] + x + col,
                        a * b,
                        da * b * p[4] as f32,
                        a * db * p[5] as f32,
                    );
                }
            }
        }
        Ok(sample)
    }
}
