use super::canvas::{
    GdiCanvas32,
    bitmap::{CompositeMode32, CompositeRequest32},
    gradient::{GradientMesh32, GradientVertex32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::gdi::GdiRuntime32,
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::{Point32, Rect32},
    },
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn msimg_bitmap_call(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let mode = match api {
            GdiApi32::AlphaBlend => {
                let [operation, flags, constant, alpha] = args[10].to_le_bytes();
                if operation != 0 || flags != 0 || alpha > 1 {
                    return Err(GdiError32::InvalidRequest);
                }
                CompositeMode32::Alpha {
                    constant,
                    pixel_alpha: alpha == 1,
                }
            }
            GdiApi32::TransparentBlt => CompositeMode32::Transparent(args[10]),
            _ => return Err(GdiError32::InvalidRequest),
        };
        let request = CompositeRequest32 {
            destination: rectangle(args[1], args[2], args[3], args[4])?,
            source: args[5],
            source_area: rectangle(args[6], args[7], args[8], args[9])?,
            mode,
        };
        let mut canvas = GdiCanvas32::new(self, memory, args[0])?;
        canvas.composite(self, memory, request)?;
        self.publish_canvas(memory, canvas)?;
        Ok(1)
    }

    pub(in crate::process::windows::gdi) fn gradient_call(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if args[5] > 2 || args[2] == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let mesh_stride = if args[5] == 2 { 12 } else { 8 };
        let vertex_bytes = (args[2] as usize)
            .checked_mul(16)
            .ok_or(GdiError32::Capacity)?;
        let mesh_bytes = (args[4] as usize)
            .checked_mul(mesh_stride)
            .ok_or(GdiError32::Capacity)?;
        if vertex_bytes
            .checked_add(mesh_bytes)
            .ok_or(GdiError32::Capacity)?
            > self.config.bitmap_bytes
        {
            return Err(GdiError32::Capacity);
        }
        let mut raw_vertices = super::super::memory::zeroes(vertex_bytes)?;
        memory.read(GuestAddress(args[1]), &mut raw_vertices)?;
        let mut raw_meshes = super::super::memory::zeroes(mesh_bytes)?;
        if mesh_bytes != 0 {
            memory.read(GuestAddress(args[3]), &mut raw_meshes)?;
        }
        let mut vertices = Vec::new();
        vertices
            .try_reserve_exact(args[2] as usize)
            .map_err(|_| GdiError32::Allocation)?;
        for raw in raw_vertices.chunks_exact(16) {
            vertices.push(GradientVertex32 {
                point: Point32 {
                    x: i32::from_le_bytes(raw[..4].try_into().unwrap()),
                    y: i32::from_le_bytes(raw[4..8].try_into().unwrap()),
                },
                color: core::array::from_fn(|i| {
                    u16::from_le_bytes(raw[8 + i * 2..10 + i * 2].try_into().unwrap())
                }),
            });
        }
        let mut meshes = Vec::new();
        meshes
            .try_reserve_exact(args[4] as usize)
            .map_err(|_| GdiError32::Allocation)?;
        for raw in raw_meshes.chunks_exact(mesh_stride) {
            let index = |i: usize| u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
            let mesh = if args[5] == 2 {
                GradientMesh32::Triangle([index(0), index(1), index(2)])
            } else {
                GradientMesh32::Rectangle {
                    vertices: [index(0), index(1)],
                    vertical: args[5] == 1,
                }
            };
            meshes.push(mesh);
        }
        let mut canvas = GdiCanvas32::new(self, memory, args[0])?;
        canvas.gradient(self, memory, &vertices, &meshes)?;
        self.publish_canvas(memory, canvas)?;
        Ok(1)
    }
}

fn rectangle(x: u32, y: u32, width: u32, height: u32) -> Result<Rect32, GdiError32> {
    let x = x as i32;
    let y = y as i32;
    let width = width as i32;
    let height = height as i32;
    if width <= 0 || height <= 0 {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(Rect32 {
        left: x,
        top: y,
        right: x.checked_add(width).ok_or(GdiError32::InvalidRequest)?,
        bottom: y.checked_add(height).ok_or(GdiError32::InvalidRequest)?,
    })
}
