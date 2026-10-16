use crate::audio::{AudioError32, allocated};

pub(super) struct Delay {
    pub samples: Vec<[f32; 2]>,
    pub cursor: usize,
}
impl Delay {
    pub fn new(frames: usize) -> Result<Self, AudioError32> {
        Ok(Self {
            samples: allocated(frames.max(2), || [0.; 2])?,
            cursor: 0,
        })
    }
    pub fn read(&self, delay: f64, channel: usize) -> f64 {
        let delay = delay.clamp(1., (self.samples.len() - 1) as f64);
        let whole = delay as usize;
        let fraction = delay - whole as f64;
        let first = (self.cursor + self.samples.len() - whole) % self.samples.len();
        let next = (first + self.samples.len() - 1) % self.samples.len();
        let first = f64::from(self.samples[first][channel]);
        first + (f64::from(self.samples[next][channel]) - first) * fraction
    }
    pub fn write(&mut self, value: [f64; 2]) {
        self.samples[self.cursor] = value.map(super::sample::stored_sample);
        self.cursor = (self.cursor + 1) % self.samples.len();
    }
}
