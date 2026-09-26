use super::{DepthPolicy, Frame, Graphics, IDENTITY_MATRIX, INVALID_CALL, Viewport, quantize_d16};
use crate::execution::{GuestMemory, MemoryError};
use std::collections::BTreeMap;

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
    z: f64,
    color: [u8; 3],
    inv_w: f64,
    uv_over_w: [[f64; 2]; 2],
}

#[derive(Clone, Copy)]
struct Bounds {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    area: f64,
}

#[derive(Clone, Copy)]
struct ClipVertex {
    position: [f64; 4],
    color: [f64; 3],
    uv: [[f64; 2]; 2],
}

impl ClipVertex {
    fn lerp(self, other: Self, t: f64) -> Self {
        Self {
            position: std::array::from_fn(|i| {
                self.position[i] + (other.position[i] - self.position[i]) * t
            }),
            color: std::array::from_fn(|i| self.color[i] + (other.color[i] - self.color[i]) * t),
            uv: std::array::from_fn(|set| {
                std::array::from_fn(|i| self.uv[set][i] + (other.uv[set][i] - self.uv[set][i]) * t)
            }),
        }
    }
}

struct SampledTexture {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

impl SampledTexture {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn sample(&self, uv: [f64; 2]) -> [u8; 3] {
        let x = ((uv[0].rem_euclid(1.0) * f64::from(self.width)).floor() as usize)
            .min(self.width as usize - 1);
        let y = ((uv[1].rem_euclid(1.0) * f64::from(self.height)).floor() as usize)
            .min(self.height as usize - 1);
        let offset = (y * self.width as usize + x) * 4;
        [
            self.bgra[offset + 2],
            self.bgra[offset + 1],
            self.bgra[offset],
        ]
    }
}

pub(super) struct VertexLayout {
    size: u32,
    color: Option<usize>,
    coordinates: usize,
    uv_offset: usize,
}

pub(super) fn vertex_layout(fvf: u32) -> Option<VertexLayout> {
    if !matches!(
        fvf,
        0x102 | 0x112 | 0x142 | 0x152 | 0x202 | 0x212 | 0x242 | 0x252
    ) {
        return None;
    }
    let before_color = if fvf & 0x10 == 0 { 12 } else { 24 };
    let color = (fvf & 0x40 != 0).then_some(before_color);
    let uv_offset = before_color + usize::from(color.is_some()) * 4;
    let coordinates = ((fvf >> 8) & 0xf) as usize;
    Some(VertexLayout {
        size: u32::try_from(uv_offset + coordinates * 8).unwrap(),
        color,
        coordinates,
        uv_offset,
    })
}

struct SampledStage {
    state: super::color::Stage,
    texture: Option<SampledTexture>,
}

fn sampled_stages(
    graphics: &Graphics,
    coordinates: usize,
    memory: &GuestMemory,
) -> Result<Option<Vec<SampledStage>>, MemoryError> {
    let mut stages = Vec::new();
    for (index, state) in graphics.color_stages.iter().copied().enumerate() {
        let address = graphics.texture_stages[index];
        if state.operation == 1 || (state.argument1 == 2 && address == 0) {
            break;
        }
        if index >= 2 {
            return Ok(None);
        }
        let texture = if state.uses_texture() {
            if state.coordinate as usize >= coordinates {
                return Ok(None);
            }
            let Some(texture) = graphics.textures.get(&address) else {
                return Ok(None);
            };
            let level = &texture.levels[0];
            let mut bgra = vec![0; level.width as usize * level.height as usize * 4];
            memory.read(u64::from(address) + u64::from(level.offset), &mut bgra)?;
            Some(SampledTexture {
                width: level.width,
                height: level.height,
                bgra,
            })
        } else {
            None
        };
        stages.push(SampledStage { state, texture });
    }
    Ok(Some(stages))
}

#[allow(clippy::too_many_lines)]
pub(super) fn draw_indexed(
    graphics: &mut Graphics,
    args: &[u32],
    memory: &GuestMemory,
) -> Result<u32, MemoryError> {
    let [
        device,
        topology,
        min_index,
        num_vertices,
        start_index,
        primitive_count,
    ] = <[u32; 6]>::try_from(args).expect("d3d8 call arity");
    let Some(viewport) = graphics.viewport else {
        return Ok(INVALID_CALL);
    };
    let Some((index_address, base_vertex)) = graphics.indices else {
        return Ok(INVALID_CALL);
    };
    let Some((vertex_address, stride)) = graphics.stream else {
        return Ok(INVALID_CALL);
    };
    let Some(layout) = vertex_layout(graphics.vertex_fvf) else {
        return Ok(INVALID_CALL);
    };
    let vertex_size = layout.size;
    if device != super::DEVICE
        || graphics.device_refs == 0
        || graphics.back.is_none()
        || topology != 4
        || primitive_count > MAX_PRIMITIVES
        || stride < vertex_size
    {
        return Ok(INVALID_CALL);
    }
    let Some(index_buffer) = graphics.index_buffers.get(&index_address) else {
        return Ok(INVALID_CALL);
    };
    let Some(vertex_buffer) = graphics.vertex_buffers.get(&vertex_address) else {
        return Ok(INVALID_CALL);
    };
    let index_size = if index_buffer.format == 101 {
        2_u32
    } else {
        4_u32
    };
    let Some(index_count) = primitive_count.checked_mul(3) else {
        return Ok(INVALID_CALL);
    };
    let Some(index_end) = start_index
        .checked_add(index_count)
        .and_then(|end| end.checked_mul(index_size))
    else {
        return Ok(INVALID_CALL);
    };
    let Some(vertex_range_end) = min_index.checked_add(num_vertices) else {
        return Ok(INVALID_CALL);
    };
    if index_end > index_buffer.lock.byte_length || (primitive_count != 0 && num_vertices == 0) {
        return Ok(INVALID_CALL);
    }
    if primitive_count == 0 {
        return Ok(0);
    }

    // snapshot every active texture before changing color or depth.
    let Some(stages) = sampled_stages(graphics, layout.coordinates, memory)? else {
        return Ok(INVALID_CALL);
    };

    let index_start = u64::from(index_address)
        + super::PAGE_SIZE
        + u64::from(start_index) * u64::from(index_size);
    let mut index_bytes = vec![0; index_count as usize * index_size as usize];
    memory.read(index_start, &mut index_bytes)?;
    let mut triangles = Vec::with_capacity(primitive_count as usize);
    let mut unique = BTreeMap::new();
    for triangle in index_bytes.chunks_exact(index_size as usize * 3) {
        let mut keys = [0_u32; 3];
        for (slot, bytes) in triangle.chunks_exact(index_size as usize).enumerate() {
            let index = if index_size == 2 {
                u32::from(u16::from_le_bytes(bytes.try_into().expect("index16")))
            } else {
                u32::from_le_bytes(bytes.try_into().expect("index32"))
            };
            if !(min_index..vertex_range_end).contains(&index) {
                return Ok(INVALID_CALL);
            }
            let Some(vertex) = base_vertex.checked_add(index) else {
                return Ok(INVALID_CALL);
            };
            let Some(end) = vertex
                .checked_mul(stride)
                .and_then(|start| start.checked_add(vertex_size))
            else {
                return Ok(INVALID_CALL);
            };
            if end > vertex_buffer.lock.byte_length {
                return Ok(INVALID_CALL);
            }
            keys[slot] = vertex;
        }
        triangles.push(keys);
    }

    let world = graphics.transforms.get(&256).unwrap_or(&IDENTITY_MATRIX);
    let view = graphics.transforms.get(&2).unwrap_or(&IDENTITY_MATRIX);
    let projection = graphics.transforms.get(&3).unwrap_or(&IDENTITY_MATRIX);
    let vertex_base = u64::from(vertex_address) + super::PAGE_SIZE;
    for key in triangles.iter().flat_map(|triangle| triangle.iter()) {
        if unique.contains_key(key) {
            continue;
        }
        let mut bytes = [0; 44];
        memory.read(
            vertex_base + u64::from(*key) * u64::from(stride),
            &mut bytes[..vertex_size as usize],
        )?;
        let xyz = std::array::from_fn::<_, 3, _>(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("position"))
        });
        // normals occupy layout space but are unused by the diffuse-only raster path.
        let mut uv = [[0.0; 2]; 2];
        for (set, values) in uv.iter_mut().enumerate().take(layout.coordinates) {
            for (i, value) in values.iter_mut().enumerate() {
                let offset = layout.uv_offset + set * 8 + i * 4;
                *value =
                    f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("texcoord"));
            }
        }
        if xyz
            .iter()
            .chain(uv.iter().flatten())
            .any(|value| !value.is_finite())
        {
            return Ok(INVALID_CALL);
        }
        let mut position = [f64::from(xyz[0]), f64::from(xyz[1]), f64::from(xyz[2]), 1.0];
        for matrix in [world, view, projection] {
            position = transform(position, matrix);
        }
        if position.iter().any(|value| !value.is_finite()) {
            return Ok(INVALID_CALL);
        }
        unique.insert(
            *key,
            ClipVertex {
                position,
                color: layout.color.map_or([255.0; 3], |offset| {
                    [
                        f64::from(bytes[offset + 2]),
                        f64::from(bytes[offset + 1]),
                        f64::from(bytes[offset]),
                    ]
                }),
                uv: uv.map(|set| set.map(f64::from)),
            },
        );
    }

    let mut prepared = Vec::new();
    let mut samples = 0_u64;
    for triangle in triangles {
        let polygon = clip_triangle(&[
            unique[&triangle[0]],
            unique[&triangle[1]],
            unique[&triangle[2]],
        ]);
        for slot in 1..polygon.len().saturating_sub(1) {
            let Some(a) = screen_vertex(polygon[0], viewport) else {
                continue;
            };
            let Some(b) = screen_vertex(polygon[slot], viewport) else {
                continue;
            };
            let Some(c) = screen_vertex(polygon[slot + 1], viewport) else {
                continue;
            };
            if let Some(bounds) = triangle_bounds(viewport, a, b, c) {
                samples += ((bounds.right - bounds.left) * (bounds.bottom - bounds.top)) as u64;
                if samples > MAX_RASTER_SAMPLES {
                    return Ok(INVALID_CALL);
                }
                prepared.push(([a, b, c], bounds));
            }
        }
    }

    let frame = graphics.back.as_mut().expect("validated back buffer");
    let mut depth = graphics.depth.as_deref_mut().filter(|_| graphics.z_enabled);
    for ([a, b, c], bounds) in prepared {
        raster_triangle(
            frame,
            depth.as_deref_mut(),
            graphics.depth_policy,
            [a, b, c],
            bounds,
            &stages,
        );
    }
    Ok(0)
}

fn transform(position: [f64; 4], matrix: &[u32; 16]) -> [f64; 4] {
    std::array::from_fn(|column| {
        (0..4)
            .map(|row| position[row] * f64::from(f32::from_bits(matrix[row * 4 + column])))
            .sum()
    })
}

fn clip_triangle(vertices: &[ClipVertex; 3]) -> Vec<ClipVertex> {
    let mut polygon = vertices.to_vec();
    for plane in [
        [1.0, 0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, -1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0, 1.0],
    ] {
        if polygon.is_empty() {
            break;
        }
        let mut clipped = Vec::with_capacity(polygon.len() + 1);
        let mut previous = *polygon.last().expect("nonempty polygon");
        let mut old_distance = plane_distance(previous, plane);
        for current in polygon {
            let distance = plane_distance(current, plane);
            if (old_distance >= 0.0) != (distance >= 0.0) {
                let t = old_distance / (old_distance - distance);
                clipped.push(previous.lerp(current, t));
            }
            if distance >= 0.0 {
                clipped.push(current);
            }
            previous = current;
            old_distance = distance;
        }
        polygon = clipped;
    }
    polygon
}

fn plane_distance(vertex: ClipVertex, plane: [f64; 4]) -> f64 {
    (0..4)
        .map(|index| vertex.position[index] * plane[index])
        .sum()
}

// Viewport coordinates are bounded by the admitted one-megapixel render target.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn screen_vertex(clip: ClipVertex, viewport: Viewport) -> Option<Vertex> {
    let w = clip.position[3];
    if w <= 0.0 {
        return None;
    }
    let width = (viewport.right - viewport.left) as f64;
    let height = (viewport.bottom - viewport.top) as f64;
    let x = viewport.left as f64 + (clip.position[0] / w + 1.0) * width * 0.5;
    let y = viewport.top as f64 + (1.0 - clip.position[1] / w) * height * 0.5;
    let z = clip.position[2] / w;
    (x.is_finite() && y.is_finite() && z.is_finite()).then_some(Vertex {
        x,
        y,
        z,
        color: clip
            .color
            .map(|channel| channel.round().clamp(0.0, 255.0) as u8),
        inv_w: 1.0 / w,
        uv_over_w: clip.uv.map(|set| set.map(|coord| coord / w)),
    })
}

pub(super) fn draw_up(
    frame: Option<&mut Frame>,
    mut depth: Option<&mut [u16]>,
    depth_policy: DepthPolicy,
    viewport: Option<Viewport>,
    fvf: u32,
    args: &[u32],
    memory: &GuestMemory,
) -> Result<u32, MemoryError> {
    let Some(frame) = frame else {
        return Ok(INVALID_CALL);
    };
    let Some(viewport) = viewport else {
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
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || !(0.0..=1.0).contains(&z)
            || !rhw.is_finite()
            || rhw <= 0.0
        {
            return Ok(INVALID_CALL);
        }
        vertices.push(Vertex {
            x: f64::from(x),
            y: f64::from(y),
            z: f64::from(z),
            color: [bytes[18], bytes[17], bytes[16]],
            inv_w: 1.0,
            uv_over_w: [[0.0; 2]; 2],
        });
    }

    let mut triangles = Vec::with_capacity(count as usize);
    let mut samples = 0_u64;
    for primitive in 0..count as usize {
        let indices = primitive_indices(topology, primitive);
        let bounds = triangle_bounds(
            viewport,
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
                depth.as_deref_mut(),
                depth_policy,
                [
                    vertices[indices[0]],
                    vertices[indices[1]],
                    vertices[indices[2]],
                ],
                bounds,
                &[],
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

// Finite coordinates saturate on integer conversion, then clip to the owned viewport.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn triangle_bounds(viewport: Viewport, a: Vertex, b: Vertex, c: Vertex) -> Option<Bounds> {
    let area = edge(a, b, c.x, c.y);
    // the default D3D cull mode rejects counterclockwise screen-space triangles.
    if area <= 0.0 || !area.is_finite() {
        return None;
    }
    let left = (a.x.min(b.x).min(c.x).floor() as usize).max(viewport.left);
    let top = (a.y.min(b.y).min(c.y).floor() as usize).max(viewport.top);
    let right = (a.x.max(b.x).max(c.x).ceil() as usize).min(viewport.right);
    let bottom = (a.y.max(b.y).max(c.y).ceil() as usize).min(viewport.bottom);
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
fn raster_triangle(
    frame: &mut Frame,
    mut depth: Option<&mut [u16]>,
    depth_policy: DepthPolicy,
    [a, b, c]: [Vertex; 3],
    bounds: Bounds,
    stages: &[SampledStage],
) {
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
            let pixel_index = y * frame.width as usize + x;
            if let Some(depth) = depth.as_deref_mut() {
                let z = quantize_d16((wa * a.z + wb * b.z + wc * c.z).clamp(0.0, 1.0));
                if !depth_policy.passes(z, depth[pixel_index]) {
                    continue;
                }
                if depth_policy.write_enabled {
                    depth[pixel_index] = z;
                }
            }
            let offset = pixel_index * 4;
            let diffuse = std::array::from_fn(|channel| {
                wa * f64::from(a.color[channel])
                    + wb * f64::from(b.color[channel])
                    + wc * f64::from(c.color[channel])
            });
            let mut current = diffuse;
            for stage in stages {
                let sampled = stage.texture.as_ref().map_or([255; 3], |texture| {
                    let inv_w = wa * a.inv_w + wb * b.inv_w + wc * c.inv_w;
                    let set = stage.state.coordinate as usize;
                    let uv = std::array::from_fn(|i| {
                        (wa * a.uv_over_w[set][i]
                            + wb * b.uv_over_w[set][i]
                            + wc * c.uv_over_w[set][i])
                            / inv_w
                    });
                    texture.sample(uv)
                });
                current = stage.state.combine(diffuse, current, sampled);
            }
            for (channel, value) in current.into_iter().enumerate() {
                frame.rgba[offset + channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            frame.rgba[offset + 3] = 255;
        }
    }
}

fn edge(a: Vertex, b: Vertex, x: f64, y: f64) -> f64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}
