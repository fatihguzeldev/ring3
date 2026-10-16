#[derive(Default)]
pub(super) struct Filter {
    coefficients: [f64; 5],
    state: [[f64; 2]; 2],
}
impl Filter {
    pub fn peak(&mut self, rate: u32, frequency: f64, semitones: f64, gain: f64) {
        let a = 10.0f64.powf(gain / 40.);
        let omega = core::f64::consts::TAU * (frequency / rate as f64).min(0.49);
        let sine = omega.sin();
        let alpha =
            sine * (core::f64::consts::LN_2 * semitones / 24. * omega / sine.max(1e-8)).sinh();
        let inverse = 1. / (1. + alpha / a);
        self.coefficients = [
            (1. + alpha * a) * inverse,
            -2. * omega.cos() * inverse,
            (1. - alpha * a) * inverse,
            -2. * omega.cos() * inverse,
            (1. - alpha / a) * inverse,
        ];
    }
    pub fn process(&mut self, value: [f64; 2]) -> [f64; 2] {
        let [b0, b1, b2, a1, a2] = self.coefficients;
        core::array::from_fn(|channel| {
            let output = b0 * value[channel] + self.state[channel][0];
            self.state[channel][0] = b1 * value[channel] - a1 * output + self.state[channel][1];
            self.state[channel][1] = b2 * value[channel] - a2 * output;
            output
        })
    }
}
