use super::*;
use rustfft::num_complex::Complex32 as VendorComplex;

fn close(actual: f32, expected: f64, length: usize) {
    let bound = 16. * f64::from(f32::EPSILON) * length as f64;
    assert!(
        (f64::from(actual) - expected).abs() <= bound,
        "{actual} != {expected}, bound {bound}"
    );
}

#[test]
fn selected_spectrum_matches_independent_dft_and_inverse_preserves_neighbor_rows() {
    for length in [8, 17, 256] {
        let plan = Plan32::new(length);
        let mut workspace = plan.workspace().unwrap();
        let mut buffer = Buffer32::zeroed(length + 2).unwrap();
        let sentinel = Complex32::new(-0., f32::from_bits(0x3f12_3456));
        buffer.set(0, sentinel);
        buffer.set(length + 1, sentinel);
        let input = (0..length)
            .map(|index| Complex32::new((index % 7) as f32 / 8., (index % 3) as f32 / 16.))
            .collect::<Vec<_>>();
        for (index, value) in input.iter().enumerate() {
            buffer.set(index + 1, *value);
        }
        plan.forward(&mut buffer, 1..length + 1, &mut workspace);
        for bin in 0..length {
            let mut expected = [0.; 2];
            for (index, value) in input.iter().enumerate() {
                let phase = -core::f64::consts::TAU * (bin * index) as f64 / length as f64;
                let (sine, cosine) = phase.sin_cos();
                expected[0] += f64::from(value.re) * cosine - f64::from(value.im) * sine;
                expected[1] += f64::from(value.re) * sine + f64::from(value.im) * cosine;
            }
            let actual = buffer.get(bin + 1);
            close(actual.re, expected[0], length);
            close(actual.im, expected[1], length);
        }
        plan.inverse(&mut buffer, 1..length + 1, &mut workspace);
        buffer.divide(1..length + 1, length as f32);
        for (index, expected) in input.iter().enumerate() {
            let actual = buffer.get(index + 1);
            close(actual.re, f64::from(expected.re), length);
            close(actual.im, f64::from(expected.im), length);
        }
        for index in [0, length + 1] {
            assert_eq!(buffer.get(index).re.to_bits(), sentinel.re.to_bits());
            assert_eq!(buffer.get(index).im.to_bits(), sentinel.im.to_bits());
        }
    }
}

#[test]
fn repeated_transforms_keep_the_same_buffer_and_workspace_storage() {
    let plan = Plan32::new(256);
    let mut workspace = plan.workspace().unwrap();
    let mut buffer = Buffer32::zeroed(512).unwrap();
    let buffer_storage = (buffer.values.as_ptr(), buffer.values.capacity());
    let scratch_storage = (
        workspace.values.values.as_ptr(),
        workspace.values.values.capacity(),
    );
    for iteration in 0..1000 {
        let first = (iteration % 2) * 256;
        buffer.clear_range(first..first + 256);
        buffer.set(first + 7, Complex32::new(1., 0.));
        plan.forward(&mut buffer, first..first + 256, &mut workspace);
        plan.inverse(&mut buffer, first..first + 256, &mut workspace);
        buffer.divide(first..first + 256, 256.);
        close(buffer.get(first + 7).re, 1., 256);
        assert_eq!(
            (buffer.values.as_ptr(), buffer.values.capacity()),
            buffer_storage
        );
        assert_eq!(
            (
                workspace.values.values.as_ptr(),
                workspace.values.values.capacity()
            ),
            scratch_storage
        );
    }
}

#[test]
fn complex_arithmetic_preserves_the_existing_vendor_operation_order_and_bits() {
    for (re, im) in [(0., -0.), (1.25, -0.75), (f32::MIN_POSITIVE, -3.5)] {
        let value = Complex32::new(re, im);
        let other = Complex32::new(-0.375, 2.5);
        let vendor = VendorComplex::new(re, im);
        let product = vendor * VendorComplex::new(other.re, other.im) * 0.25;
        let actual = value * other * 0.25;
        assert_eq!(actual.re.to_bits(), product.re.to_bits());
        assert_eq!(actual.im.to_bits(), product.im.to_bits());
        assert_eq!(value.norm_sqr().to_bits(), vendor.norm_sqr().to_bits());
        assert_eq!(value.conj().im.to_bits(), vendor.conj().im.to_bits());
        let mut buffer = Buffer32::zeroed(1).unwrap();
        buffer.set(0, value);
        buffer.add(0, other);
        buffer.scale(0, 0.75);
        buffer.divide(0..1, 256.);
        let expected = (vendor + VendorComplex::new(other.re, other.im)) * 0.75 / 256.;
        assert_eq!(buffer.get(0).re.to_bits(), expected.re.to_bits());
        assert_eq!(buffer.get(0).im.to_bits(), expected.im.to_bits());
        buffer.clear();
        assert_eq!(buffer.get(0).re.to_bits(), 0);
        assert_eq!(buffer.get(0).im.to_bits(), 0);
    }
    assert_eq!(Buffer32::VALUE_BYTES, 8);
    assert!(matches!(Buffer32::zeroed(usize::MAX), Err(AllocationError)));
}
