use super::*;

fn high_frequency_tail(ratio: f32) -> f64 {
    let mut chain = EffectChain::prepare(&[EffectKind::I3dl2Reverb], 1000, 1 << 20).unwrap();
    let words = [
        0,
        0,
        0.0f32.to_bits(),
        0.5f32.to_bits(),
        ratio.to_bits(),
        (-10000i32) as u32,
        0.0f32.to_bits(),
        0,
        0.0f32.to_bits(),
        0.0f32.to_bits(),
        100.0f32.to_bits(),
        80.0f32.to_bits(),
    ];
    let parameters = Parameters::parse(EffectKind::I3dl2Reverb, &words, 1000)
        .unwrap()
        .0;
    chain.set_parameters(0, parameters).unwrap();
    let mut energy = 0.;
    let mut previous = 0.;
    for frame in 0..1500 {
        let output = chain.process(if frame < 8 {
            [if frame % 2 == 0 { 0.5 } else { -0.5 }; 2]
        } else {
            [0.; 2]
        });
        assert!(output.iter().all(|sample| sample.is_finite()));
        if frame >= 250 {
            energy += (f64::from(output[0]) - previous).powi(2);
        }
        previous = f64::from(output[0]);
    }
    energy
}

#[test]
fn high_frequency_decay_ratio_above_one_produces_a_longer_measurable_tail() {
    let short = high_frequency_tail(0.5);
    let equal = high_frequency_tail(1.);
    let long = high_frequency_tail(2.);
    assert!(short > 0.);
    assert!(short < equal, "short={short}, equal={equal}");
    assert!(equal < long, "equal={equal}, long={long}");
}
