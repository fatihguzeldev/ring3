use super::{GdiCanvas32, bitmap::write_alpha, intersect, union};
use crate::{
    memory::AddressSpace,
    process::windows::gdi::GdiRuntime32,
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32},
    },
};

pub(in crate::process::windows::gdi) struct GradientVertex32 {
    pub point: Point32,
    pub color: [u16; 3],
}
pub(in crate::process::windows::gdi) enum GradientMesh32 {
    Rectangle { vertices: [u32; 2], vertical: bool },
    Triangle([u32; 3]),
}

impl GdiCanvas32 {
    pub(in crate::process::windows::gdi) fn gradient(
        &mut self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        vertices: &[GradientVertex32],
        meshes: &[GradientMesh32],
    ) -> Result<(), GdiError32> {
        let prepared = self.prepared.as_ref().ok_or(GdiError32::InvalidRequest)?;
        self.owner.check(gdi, memory, self.dc, prepared.index)?;
        let mut points = Vec::new();
        points
            .try_reserve_exact(vertices.len())
            .map_err(|_| GdiError32::Allocation)?;
        for vertex in vertices {
            points.push(prepared.state.device_point(vertex.point)?);
        }
        for mesh in meshes {
            let indices: &[u32] = match mesh {
                GradientMesh32::Rectangle { vertices, .. } => vertices,
                GradientMesh32::Triangle(vertices) => vertices,
            };
            if indices
                .iter()
                .any(|&index| index as usize >= vertices.len())
            {
                return Err(GdiError32::InvalidRequest);
            }
        }
        let prepared = self.prepared.as_mut().unwrap();
        let bounds = super::super::super::bitmaps::bounds(self.owner.layout);
        for mesh in meshes {
            let (indices, area) = match mesh {
                GradientMesh32::Rectangle {
                    vertices: [a, b], ..
                } => {
                    let first = points[*a as usize];
                    let last = points[*b as usize];
                    (
                        [*a, *b, *b],
                        Rect32 {
                            left: first.x,
                            top: first.y,
                            right: last.x,
                            bottom: last.y,
                        },
                    )
                }
                GradientMesh32::Triangle(indices) => {
                    let [a, b, c] = indices.map(|index| points[index as usize]);
                    (
                        *indices,
                        Rect32 {
                            left: a.x.min(b.x).min(c.x),
                            top: a.y.min(b.y).min(c.y),
                            right: a.x.max(b.x).max(c.x),
                            bottom: a.y.max(b.y).max(c.y),
                        },
                    )
                }
            };
            let area = intersect(area, bounds);
            let [a, b, c] = indices.map(|index| points[index as usize]);
            let triangle_area = edge(a, b, c);
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let point = Point32 { x, y };
                    if !prepared.state.accepts(point, bounds) {
                        continue;
                    }
                    let (weights, divisor) = match mesh {
                        GradientMesh32::Rectangle { vertical, .. } => {
                            let (value, first, last) = if *vertical {
                                (y, a.y, b.y)
                            } else {
                                (x, a.x, b.x)
                            };
                            let end = i128::from(last) - i128::from(first);
                            if end <= 0 {
                                continue;
                            }
                            let position = i128::from(value) - i128::from(first);
                            ([end - position, position, 0], end)
                        }
                        GradientMesh32::Triangle(_) => {
                            if triangle_area == 0 {
                                continue;
                            }
                            let sign = triangle_area.signum();
                            let weights = [
                                edge(b, c, point) * sign,
                                edge(c, a, point) * sign,
                                edge(a, b, point) * sign,
                            ];
                            if weights.iter().any(|&weight| weight < 0) {
                                continue;
                            }
                            (weights, triangle_area.abs())
                        }
                    };
                    let mut color = 0;
                    for channel in 0..3 {
                        let numerator: i128 = (0..3)
                            .map(|i| {
                                i128::from(vertices[indices[i] as usize].color[channel])
                                    * weights[i]
                            })
                            .sum();
                        color |= ((numerator / divisor) as u32 >> 8) << (channel * 8);
                    }
                    self.owner.layout.write_pixel(
                        &mut prepared.pixels,
                        &self.owner.colors,
                        x as u32,
                        y as u32,
                        color,
                    );
                    write_alpha(
                        self.owner.layout,
                        &mut prepared.pixels,
                        x as u32,
                        y as u32,
                        255,
                    );
                    prepared.damage = union(
                        prepared.damage,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                    );
                }
            }
        }
        Ok(())
    }
}

fn edge(a: Point32, b: Point32, c: Point32) -> i128 {
    (i128::from(b.x) - i128::from(a.x)) * (i128::from(c.y) - i128::from(a.y))
        - (i128::from(b.y) - i128::from(a.y)) * (i128::from(c.x) - i128::from(a.x))
}
