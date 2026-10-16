use super::*;
use crate::graphics::d3d8::{
    patch::{Degree, RectPatchInfo, TriPatchInfo},
    shader::{Element, ElementType, Tessellation},
};

fn rect(basis: Basis, degree: Degree, width: u32) -> SurfaceProgram {
    SurfaceProgram::compile(PatchInfo::Rectangle(RectPatchInfo {
        start_width: 2,
        start_height: 1,
        width,
        height: width,
        stride: width + 2,
        basis,
        degree,
    }))
    .unwrap()
}
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.00003,
        "{actual} != {expected}"
    );
}

#[test]
fn bernstein_coefficients_control_order_and_analytic_derivatives_are_explicit() {
    let p = rect(Basis::Bezier, Degree::Cubic, 4);
    assert_eq!(
        &p.words()[8..14]
            .iter()
            .map(|v| f32::from_bits(*v))
            .collect::<Vec<_>>(),
        &[1.0, -3.0, 3.0, -1.0, 0.0, 0.0]
    );
    assert_eq!(
        &p.words()[14..20]
            .iter()
            .map(|v| f32::from_bits(*v))
            .collect::<Vec<_>>(),
        &[0.0, 3.0, -6.0, 3.0, 0.0, 0.0]
    );
    assert_eq!(p.source_vertex(0), Some(8));
    assert_eq!(p.source_vertex(15), Some(29));
    assert_eq!(p.source_vertex(16), None);
    for degree in [Degree::Linear, Degree::Cubic, Degree::Quintic] {
        let n = degree as u32;
        let p = rect(Basis::Bezier, degree, n + 1);
        let control = |i: u32| {
            [
                (i % (n + 1)) as f32 / n as f32,
                (i / (n + 1)) as f32 / n as f32,
                2.0,
                1.0,
            ]
        };
        for uv in [[0.0, 0.0], [1.0, 1.0], [0.23, 0.71]] {
            let s = p.evaluate(uv, control).unwrap();
            close(s.value[0], uv[0]);
            close(s.value[1], uv[1]);
            close(s.value[2], 2.0);
            close(s.du[0], 1.0);
            close(s.dv[1], 1.0);
            for (a, b) in s.normal().into_iter().zip([0.0, 0.0, 1.0, 1.0]) {
                close(a, b);
            }
        }
    }
}

#[test]
fn triangular_monomials_match_literal_row_order_and_multinomial_weights() {
    for degree in [Degree::Linear, Degree::Cubic, Degree::Quintic] {
        let n = degree as u32;
        let count = (n + 1) * (n + 2) / 2;
        let p = SurfaceProgram::compile(PatchInfo::Triangle(TriPatchInfo {
            start: 9,
            vertices: count,
            basis: Basis::Bezier,
            degree,
        }))
        .unwrap();
        assert_eq!(&p.words()[8..12], &[0, 0, n, 1.0f32.to_bits()]);
        let mut controls = [[0.0; 4]; 21];
        let mut i = 0;
        for row in 0..=n {
            for col in 0..=row {
                controls[i] = [
                    (row - col) as f32 / n as f32,
                    col as f32 / n as f32,
                    0.0,
                    1.0,
                ];
                i += 1;
            }
        }
        for uv in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.2, 0.3]] {
            let s = p.evaluate(uv, |i| controls[i as usize]).unwrap();
            close(s.value[0], uv[0]);
            close(s.value[1], uv[1]);
            close(s.du[0], 1.0);
            close(s.dv[1], 1.0);
            close(s.du[3], 0.0);
            close(s.dv[3], 0.0);
        }
        assert_eq!(p.source_vertex(count - 1), Some(9 + count - 1));
    }
}

#[test]
fn cardinal_and_catmull_rom_windows_preserve_partition_and_join_derivatives() {
    for degree in [Degree::Linear, Degree::Cubic, Degree::Quintic] {
        let n = degree as u32;
        let width = n + 3;
        let p = rect(Basis::BSpline, degree, width);
        for uv in [[0.0, 0.0], [0.31, 0.6], [1.0, 1.0]] {
            let sample = p.evaluate(uv, |_| [1.0, 2.0, 3.0, 4.0]).unwrap();
            for (value, expected) in sample.value.into_iter().zip([1.0, 2.0, 3.0, 4.0]) {
                close(value, expected);
            }
            for derivative in sample.du.into_iter().chain(sample.dv) {
                close(derivative, 0.0);
            }
        }
    }
    let p = rect(Basis::Interpolate, Degree::Cubic, 5);
    let control = |i: u32| [(i % 5) as f32, (i / 5) as f32, 0.0, 1.0];
    let a = p.evaluate([0.0, 0.0], control).unwrap();
    assert_eq!(a.value, [1.0, 1.0, 0.0, 1.0]);
    let b = p.evaluate([1.0, 1.0], control).unwrap();
    assert_eq!(b.value, [3.0, 3.0, 0.0, 1.0]);
    let left = p.evaluate([0.5 - 0.00001, 0.4], control).unwrap();
    let right = p.evaluate([0.5 + 0.00001, 0.4], control).unwrap();
    close(left.du[0], right.du[0]);
    close(left.dv[1], right.dv[1]);
}

#[test]
fn generated_normal_matches_independent_finite_difference_and_zero_surface() {
    let p = rect(Basis::Bezier, Degree::Cubic, 4);
    let control = |i: u32| {
        let x = (i % 4) as f32;
        let y = (i / 4) as f32;
        [x, y, x * y, 1.0]
    };
    let sample = p.evaluate([0.2, 0.4], control).unwrap();
    let a = p.evaluate([0.199, 0.4], control).unwrap();
    let b = p.evaluate([0.201, 0.4], control).unwrap();
    for lane in 0..3 {
        assert!((sample.du[lane] - (b.value[lane] - a.value[lane]) / 0.002).abs() < 0.001);
    }
    assert_eq!(
        p.evaluate([0.3, 0.7], |_| [0.0; 4]).unwrap().normal(),
        [0.0, 0.0, 0.0, 1.0]
    );
}

#[test]
fn input_plan_lowering_validates_generated_register_dependencies() {
    let elements = [Element {
        stream: 3,
        offset: 7,
        register: 0,
        kind: ElementType::Float3,
    }];
    let plan = InputPlan::compile(
        &elements,
        &[
            Tessellation::Normal {
                input: 0,
                output: 3,
            },
            Tessellation::Coordinates { output: 7 },
        ],
    )
    .unwrap();
    assert_eq!(&plan.words()[..8], &[1, 2, 0, 0, 7, 3, 2, 0]);
    assert_eq!(&plan.words()[72..78], &[0, 0, 3, 1, 0, 7]);
    assert!(
        InputPlan::compile(
            &elements,
            &[Tessellation::Normal {
                input: 1,
                output: 3
            }]
        )
        .is_err()
    );
    assert!(InputPlan::compile(&elements, &[Tessellation::Coordinates { output: 0 }]).is_err());
}
