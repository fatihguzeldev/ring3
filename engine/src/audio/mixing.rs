pub(super) trait MixSample: Copy {
    const SILENCE: Self;

    fn accumulate(&mut self, first: f32, next: f32, phase: u32, gain: f32);
    fn clipped(self) -> f32;
}

impl MixSample for f32 {
    const SILENCE: Self = 0.0;

    fn accumulate(&mut self, first: f32, next: f32, phase: u32, gain: f32) {
        let fraction = phase as f32 / 4_294_967_296.0;
        *self += (first + (next - first) * fraction) * gain;
    }

    fn clipped(self) -> f32 {
        self.clamp(-1.0, 1.0)
    }
}

impl MixSample for f64 {
    const SILENCE: Self = 0.0;

    fn accumulate(&mut self, first: f32, next: f32, phase: u32, gain: f32) {
        let fraction = f64::from(phase) / 4_294_967_296.0;
        let first = f64::from(first);
        let next = f64::from(next);
        *self += (first + (next - first) * fraction) * f64::from(gain);
    }

    fn clipped(self) -> f32 {
        self.clamp(-1.0, 1.0) as f32
    }
}
