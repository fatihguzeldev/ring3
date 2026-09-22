use super::{Frame, INVALID_CALL};
use crate::execution::{GuestMemory, MemoryError};

pub(super) const MAX_PRIMITIVES: u32 = 4096;
pub(super) const MAX_STRIDE: u32 = 256;
const VERTEX_SIZE: usize = 20;
const VERTEX_SIZE_U32: u32 = 20;
const FVF_XYZRHW_DIFFUSE: u32 = 0x44;
const MAX_RASTER_SAMPLES: u64 = 4_194_304;

#[derive(Clone, Copy)]
struct Vertex {
    x: f64,
    y: f64,
    color: [u8; 3],
}

#[derive(Clone, Copy)]
struct Bounds {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    area: f64,
}

pub(super) fn draw_up(
    frame: Option<&mut Frame>,
    fvf: u32,
    args: &[u32],
    memory: &GuestMemory,
) -> Result<u32, MemoryError> {
    let Some(frame) = frame else {
        return Ok(INVALID_CALL);
    };
    let [_, topology, count, pointer, stride] =
        <[u32; 5]>::try_from(args).expect("d3d8 call arity");
    if fvf != FVF_XYZRHW_DIFFUSE
        || !matches!(topology, 4..=6)
        || count > MAX_PRIMITIVES
        || !(VERTEX_SIZE_U32..=MAX_STRIDE).contains(&stride)
        || (count != 0 && pointer == 0)
    {
        return Ok(INVALID_CALL);
    }
    if count == 0 {
        return Ok(0);
    }

    let vertex_count = if topology == 4 { count * 3 } else { count + 2 };
    let Some(end) = u64::from(pointer)
        .checked_add(u64::from(vertex_count - 1) * u64::from(stride))
        .and_then(|address| address.checked_add(VERTEX_SIZE as u64))
    else {
        return Ok(INVALID_CALL);
    };
    if end > u64::from(u32::MAX) + 1 {
        return Ok(INVALID_CALL);
    }

    // read every vertex before touching the owned back buffer.
    let mut vertices = Vec::with_capacity(vertex_count as usize);
    for index in 0..vertex_count {
        let address = u64::from(pointer) + u64::from(index) * u64::from(stride);
        let mut bytes = [0; VERTEX_SIZE];
        memory.read(address, &mut bytes)?;
        let x = f32::from_le_bytes(bytes[0..4].try_into().expect("position word"));
        let y = f32::from_le_bytes(bytes[4..8].try_into().expect("position word"));
        let z = f32::from_le_bytes(bytes[8..12].try_into().expect("position word"));
        let rhw = f32::from_le_bytes(bytes[12..16].try_into().expect("position word"));
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !rhw.is_finite() || rhw <= 0.0 {
            return Ok(INVALID_CALL);
        }
        vertices.push(Vertex {
            x: f64::from(x),
            y: f64::from(y),
            color: [bytes[18], bytes[17], bytes[16]],
        });
    }

    let mut triangles = Vec::with_capacity(count as usize);
    let mut samples = 0_u64;
    for primitive in 0..count as usize {
        let indices = primitive_indices(topology, primitive);
        let bounds = triangle_bounds(
            frame,
            vertices[indices[0]],
            vertices[indices[1]],
            vertices[indices[2]],
        );
        if let Some(bounds) = bounds {
            samples += ((bounds.right - bounds.left) * (bounds.bottom - bounds.top)) as u64;
            if samples > MAX_RASTER_SAMPLES {
                return Ok(INVALID_CALL);
            }
        }
        triangles.push((indices, bounds));
    }
    for (indices, bounds) in triangles {
        if let Some(bounds) = bounds {
            raster_triangle(
                frame,
                vertices[indices[0]],
                vertices[indices[1]],
                vertices[indices[2]],
                bounds,
            );
        }
    }
    Ok(0)
}

fn primitive_indices(topology: u32, primitive: usize) -> [usize; 3] {
    match topology {
        4 => [primitive * 3, primitive * 3 + 1, primitive * 3 + 2],
        5 if primitive.is_multiple_of(2) => [primitive, primitive + 1, primitive + 2],
        5 => [primitive + 1, primitive, primitive + 2],
        6 => [0, primitive + 1, primitive + 2],
        _ => unreachable!("validated triangle topology"),
    }
}

// finite coordinates are clipped to the bounded frame before integer conversion.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn triangle_bounds(frame: &Frame, a: Vertex, b: Vertex, c: Vertex) -> Option<Bounds> {
    let area = edge(a, b, c.x, c.y);
    // the default D3D cull mode rejects counterclockwise screen-space triangles.
    if area <= 0.0 || !area.is_finite() {
        return None;
    }
    let left = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
    let top = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
    let right = a.x.max(b.x).max(c.x).ceil().min(f64::from(frame.width)) as usize;
    let bottom = a.y.max(b.y).max(c.y).ceil().min(f64::from(frame.height)) as usize;
    if left >= right || top >= bottom {
        return None;
    }
    Some(Bounds {
        left,
        top,
        right,
        bottom,
        area,
    })
}

// sample positions fit the bounded frame; interpolated color is clamped to one byte.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn raster_triangle(frame: &mut Frame, a: Vertex, b: Vertex, c: Vertex, bounds: Bounds) {
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let wa = edge(b, c, px, py) / bounds.area;
            let wb = edge(c, a, px, py) / bounds.area;
            let wc = edge(a, b, px, py) / bounds.area;
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let offset = (y * frame.width as usize + x) * 4;
            for channel in 0..3 {
                let value = wa * f64::from(a.color[channel])
                    + wb * f64::from(b.color[channel])
                    + wc * f64::from(c.color[channel]);
                frame.rgba[offset + channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            frame.rgba[offset + 3] = 255;
        }
    }
}

fn edge(a: Vertex, b: Vertex, x: f64, y: f64) -> f64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}
