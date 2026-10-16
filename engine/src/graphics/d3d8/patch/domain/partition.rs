use super::EdgeStyle;

pub(super) fn count(factor: f32, style: EdgeStyle) -> u32 {
    match style {
        EdgeStyle::Discrete => factor.ceil() as u32,
        EdgeStyle::Continuous => 2 * (factor * 0.5).ceil() as u32,
    }
}

pub(super) fn knot(index: u32, count: u32, factor: f32, style: EdgeStyle) -> f32 {
    if index == 0 {
        return 0.0;
    }
    if index == count {
        return 1.0;
    }
    if index > count / 2 {
        return 1.0 - knot(count - index, count, factor, style);
    }
    match style {
        EdgeStyle::Discrete => index as f32 / count as f32,
        EdgeStyle::Continuous => 0.5 + (index as f32 - count as f32 * 0.5) / factor,
    }
}
