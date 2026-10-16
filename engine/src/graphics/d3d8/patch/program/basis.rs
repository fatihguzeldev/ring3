use super::super::Basis;

pub(super) fn factorial(value: u32) -> u32 {
    (1..=value).product()
}

fn binomial(n: u32, k: u32) -> u32 {
    factorial(n) / (factorial(k) * factorial(n - k))
}

pub(super) fn coefficients(basis: Basis, degree: u32) -> [[f32; 6]; 6] {
    let mut result = [[0.0; 6]; 6];
    match basis {
        Basis::Bezier => {
            for i in 0..=degree {
                for j in 0..=degree - i {
                    result[i as usize][(i + j) as usize] =
                        (binomial(degree, i) * binomial(degree - i, j)) as f32
                            * if j % 2 == 0 { 1.0 } else { -1.0 };
                }
            }
        }
        Basis::BSpline => {
            // uniform cardinal splines on the local interval [degree, degree + 1].
            let mut precise = [[0.0f64; 6]; 6];
            for i in 0..=degree {
                for k in 0..=degree - i {
                    let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
                    let a = (degree - i - k) as f64;
                    for power in 0..=degree {
                        let value = sign
                            * binomial(degree + 1, k) as f64
                            * binomial(degree, power) as f64
                            * a.powi((degree - power) as i32)
                            / factorial(degree) as f64;
                        precise[i as usize][power as usize] += value;
                    }
                }
            }
            for (output, coefficients) in result.iter_mut().zip(precise) {
                for (output, value) in output.iter_mut().zip(coefficients) {
                    *output = value as f32;
                }
            }
        }
        Basis::Interpolate => {
            result[0][..4].copy_from_slice(&[0.0, -0.5, 1.0, -0.5]);
            result[1][..4].copy_from_slice(&[1.0, 0.0, -2.5, 1.5]);
            result[2][..4].copy_from_slice(&[0.0, 0.5, 2.0, -1.5]);
            result[3][..4].copy_from_slice(&[0.0, 0.0, -0.5, 0.5]);
        }
    }
    result
}
