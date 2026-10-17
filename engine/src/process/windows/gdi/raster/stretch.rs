use super::drawing::BlitImage32;
use crate::windows::{gdi::GdiError32, user32::Point32};

struct Axis32 {
    first: i128,
    last: i128,
    denominator: i128,
    origin: i32,
    forward: bool,
}
fn axis(value: i32, start: i32, end: i32, source: i32, last: i32) -> Result<Axis32, GdiError32> {
    let extent = i128::from(end) - i128::from(start);
    if extent == 0 {
        return Err(GdiError32::InvalidRequest);
    }
    let index = if extent > 0 {
        i128::from(value) - i128::from(start)
    } else {
        i128::from(start) - i128::from(value) - 1
    };
    let size = (i128::from(last) - i128::from(source)).abs();
    Ok(Axis32 {
        first: index * size,
        last: (index + 1) * size,
        denominator: extent.abs(),
        origin: source,
        forward: last >= source,
    })
}
impl Axis32 {
    fn coordinate(&self, index: i128) -> i128 {
        if self.forward {
            i128::from(self.origin) + index
        } else {
            i128::from(self.origin) - index - 1
        }
    }
    fn range(&self) -> core::ops::Range<i128> {
        self.first / self.denominator..(self.last + self.denominator - 1) / self.denominator
    }
    fn valid_range(&self, size: u32) -> core::ops::Range<i128> {
        let range = self.range();
        let (first, last) = if self.forward {
            (
                -i128::from(self.origin),
                i128::from(size) - i128::from(self.origin),
            )
        } else {
            (
                i128::from(self.origin) - i128::from(size),
                i128::from(self.origin),
            )
        };
        range.start.max(first)..range.end.min(last)
    }
    fn weight(&self, index: i128) -> u64 {
        (self.last.min((index + 1) * self.denominator) - self.first.max(index * self.denominator))
            .max(0) as u64
    }
}
fn pixel(source: &BlitImage32, x: i128, y: i128, mono_colors: Option<(u32, u32)>) -> Option<u32> {
    if x < 0
        || y < 0
        || x >= i128::from(source.layout.width)
        || y >= i128::from(source.layout.height)
    {
        return None;
    }
    if source.layout.bits_per_pixel == 1
        && let Some((foreground, background)) = mono_colors
    {
        let bit =
            (source.pixels[source.layout.row(y as u32) + x as usize / 8] >> (7 - x as u32 % 8)) & 1;
        return Some(if bit == 0 { foreground } else { background });
    }
    let mut value = source
        .layout
        .read_pixel(&source.pixels, &source.palette, x as u32, y as u32);
    if source.layout.bits_per_pixel == 32 && source.layout.masks.is_none() {
        value |= u32::from(source.pixels[source.layout.row(y as u32) + x as usize * 4 + 3]) << 24;
    }
    Some(value)
}
pub(super) fn sample(
    source: &BlitImage32,
    point: Point32,
    start: Point32,
    end: Point32,
    mode: u32,
    mono_colors: Option<(u32, u32)>,
) -> Result<Option<u32>, GdiError32> {
    let x = axis(point.x, start.x, end.x, source.start.x, source.end.x)?;
    let y = axis(point.y, start.y, end.y, source.start.y, source.end.y)?;
    let mono = source.layout.bits_per_pixel == 1;
    let shrinking = x.last - x.first > x.denominator || y.last - y.first > y.denominator;
    if mode == 4 || mono && shrinking && matches!(mode, 1 | 2) {
        let mut combined = if mode == 1 { u32::MAX } else { 0 };
        let mut sums = [0u128; 4];
        let mut weight = 0u128;
        let mut any = false;
        for sy in y.valid_range(source.layout.height) {
            for sx in x.valid_range(source.layout.width) {
                let Some(value) = pixel(source, x.coordinate(sx), y.coordinate(sy), mono_colors)
                else {
                    continue;
                };
                any = true;
                if mode == 4 {
                    let w = u128::from(x.weight(sx)) * u128::from(y.weight(sy));
                    for (channel, sum) in sums.iter_mut().enumerate() {
                        *sum += u128::from((value >> (channel * 8)) & 255) * w;
                    }
                    weight += w;
                } else if mode == 1 {
                    combined &= value;
                } else {
                    combined |= value;
                }
            }
        }
        if !any {
            return Ok(None);
        }
        if mode == 4 {
            if weight == 0 {
                return Ok(None);
            }
            Ok(Some(sums.into_iter().enumerate().fold(
                0,
                |value, (channel, sum)| {
                    value | (((sum + weight / 2) / weight) as u32) << (channel * 8)
                },
            )))
        } else {
            Ok(Some(combined))
        }
    } else {
        let chosen = |axis: &Axis32| {
            if mono && shrinking {
                (axis.last - 1) / axis.denominator
            } else {
                axis.first / axis.denominator
            }
        };
        Ok(pixel(
            source,
            x.coordinate(chosen(&x)),
            y.coordinate(chosen(&y)),
            mono_colors,
        ))
    }
}

pub(super) fn quantized(color: u32, palette: &[u32], point: Point32, origin: Point32) -> u32 {
    if palette.len() < 2 {
        return palette.first().copied().unwrap_or(color);
    }
    let distance = |value: u32| {
        [0, 8, 16]
            .into_iter()
            .map(|shift| {
                let delta = ((color >> shift) & 255) as i32 - ((value >> shift) & 255) as i32;
                delta * delta
            })
            .sum::<i32>()
    };
    let mut first = (usize::MAX, i32::MAX);
    let mut second = (usize::MAX, i32::MAX);
    for (index, value) in palette.iter().enumerate() {
        let d = distance(*value);
        if d < first.1 {
            second = first;
            first = (index, d);
        } else if d < second.1 {
            second = (index, d);
        }
    }
    let a = palette[first.0];
    let b = palette[second.0];
    let mut numerator = 0i64;
    let mut denominator = 0i64;
    for shift in [0, 8, 16] {
        let av = i64::from((a >> shift) & 255);
        let bv = i64::from((b >> shift) & 255);
        numerator += (i64::from((color >> shift) & 255) - av) * (bv - av);
        denominator += (bv - av) * (bv - av);
    }
    if denominator == 0 {
        return a;
    }
    const BAYER: [[i64; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let x = (i64::from(point.x) - i64::from(origin.x)).rem_euclid(4) as usize;
    let y = (i64::from(point.y) - i64::from(origin.y)).rem_euclid(4) as usize;
    if numerator.clamp(0, denominator) * 32 > (BAYER[y][x] * 2 + 1) * denominator {
        b
    } else {
        a
    }
}
