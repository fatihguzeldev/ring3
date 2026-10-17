use super::{context::GdiRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiApi32, GdiError32, GdiObject32, Region32},
        user32::{Point32, Rect32},
    },
};
fn rectangle(args: &[u32]) -> Rect32 {
    Rect32 {
        left: args[0] as i32,
        top: args[1] as i32,
        right: args[2] as i32,
        bottom: args[3] as i32,
    }
}
impl GdiRuntime32 {
    fn region(&self, handle: u32) -> Result<Region32, GdiError32> {
        match self.model.object(handle)? {
            GdiObject32::Region(region) => Ok(region),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub(super) fn region_call(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        match api {
            GdiApi32::CreateRectRgn | GdiApi32::CreateRectRgnIndirect => {
                let rect = if api == GdiApi32::CreateRectRgn {
                    rectangle(args)
                } else {
                    memory::rect(memory, args[0])?
                };
                let prepared = self
                    .model
                    .prepare_object(GdiObject32::Region(Region32::rectangle(rect)))?;
                Ok(self.model.commit_new(prepared))
            }
            GdiApi32::SetRectRgn => {
                self.region(args[0])?;
                self.model
                    .replace_region(args[0], Region32::rectangle(rectangle(&args[1..])))?;
                Ok(1)
            }
            GdiApi32::CombineRgn => {
                self.region(args[0])?;
                let a = self.region(args[1])?;
                let b = if args[3] == 5 {
                    Region32::default()
                } else {
                    self.region(args[2])?
                };
                let result = a.combine(&b, args[3])?;
                let value = result.complexity();
                self.model.replace_region(args[0], result)?;
                Ok(value)
            }
            GdiApi32::OffsetRgn => {
                let result = self
                    .region(args[0])?
                    .offset(args[1] as i32, args[2] as i32)?;
                let value = result.complexity();
                self.model.replace_region(args[0], result)?;
                Ok(value)
            }
            GdiApi32::EqualRgn => {
                let a = self.region(args[0])?;
                let b = self.region(args[1])?;
                Ok(u32::from(a.combine(&b, 3)?.complexity() == 1))
            }
            GdiApi32::GetRgnBox => {
                let region = self.region(args[0])?;
                memory::write_rect(memory, args[1], region.bounds())?;
                Ok(region.complexity())
            }
            GdiApi32::PtInRegion => Ok(u32::from(self.region(args[0])?.contains(Point32 {
                x: args[1] as i32,
                y: args[2] as i32,
            }))),
            GdiApi32::RectInRegion => Ok(u32::from(
                self.region(args[0])?
                    .combine(&Region32::rectangle(memory::rect(memory, args[1])?), 1)?
                    .complexity()
                    != 1,
            )),
            GdiApi32::GetRegionData => {
                let region = self.region(args[0])?;
                let length = 32 + region.rects().len() * 16;
                if args[1] == 0 || args[2] == 0 {
                    return Ok(length as u32);
                }
                if (args[1] as usize) < length {
                    return Err(GdiError32::InvalidRequest);
                }
                let mut bytes = memory::zeroes(length)?;
                let bounds = region.bounds();
                for (part, value) in bytes[..32].chunks_exact_mut(4).zip([
                    32,
                    1,
                    region.rects().len() as u32,
                    (length - 32) as u32,
                    bounds.left as u32,
                    bounds.top as u32,
                    bounds.right as u32,
                    bounds.bottom as u32,
                ]) {
                    part.copy_from_slice(&value.to_le_bytes());
                }
                for (part, rect) in bytes[32..].chunks_exact_mut(16).zip(region.rects()) {
                    for (word, value) in
                        part.chunks_exact_mut(4)
                            .zip([rect.left, rect.top, rect.right, rect.bottom])
                    {
                        word.copy_from_slice(&value.to_le_bytes());
                    }
                }
                memory.write(GuestAddress(args[2]), &bytes)?;
                Ok(length as u32)
            }
            _ => Err(GdiError32::InvalidRequest),
        }
    }
    pub(super) fn clipping(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let bounds = super::bitmaps::bounds(self.surfaces[index].layout);
        let state = self.model.state(args[0])?;
        let bounds = state.drawable_bounds(bounds);
        match api {
            GdiApi32::GetClipBox => {
                let visible = state
                    .visible_region
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(|| Region32::rectangle(bounds));
                let region = state
                    .clip
                    .clone()
                    .unwrap_or_else(|| visible.clone())
                    .combine(&visible, 1)?
                    .combine(&Region32::rectangle(bounds), 1)?;
                let mut rect = region.bounds();
                if region.complexity() != 1 {
                    let a = state.logical_point(Point32 {
                        x: rect.left,
                        y: rect.top,
                    })?;
                    let b = state.logical_point(Point32 {
                        x: rect.right,
                        y: rect.bottom,
                    })?;
                    rect = Rect32 {
                        left: a.x.min(b.x),
                        top: a.y.min(b.y),
                        right: a.x.max(b.x),
                        bottom: a.y.max(b.y),
                    };
                }
                memory::write_rect(memory, args[1], rect)?;
                Ok(region.complexity())
            }
            GdiApi32::GetClipRgn => {
                let region = state.clip.clone();
                self.region(args[1])?;
                if let Some(region) = region {
                    self.model.replace_region(args[1], region)?;
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
            GdiApi32::PtVisible => {
                let point = state.device_point(Point32 {
                    x: args[1] as i32,
                    y: args[2] as i32,
                })?;
                Ok(u32::from(state.accepts(point, bounds)))
            }
            GdiApi32::RectVisible => {
                let rect = memory::rect(memory, args[1])?;
                let a = state.device_point(Point32 {
                    x: rect.left,
                    y: rect.top,
                })?;
                let b = state.device_point(Point32 {
                    x: rect.right,
                    y: rect.bottom,
                })?;
                let region = Region32::rectangle(Rect32 {
                    left: a.x.min(b.x),
                    top: a.y.min(b.y),
                    right: a.x.max(b.x),
                    bottom: a.y.max(b.y),
                });
                let visible = state
                    .visible_region
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(|| Region32::rectangle(bounds));
                let clip = state
                    .clip
                    .clone()
                    .unwrap_or_else(|| visible.clone())
                    .combine(&visible, 1)?;
                Ok(u32::from(
                    region
                        .combine(&clip, 1)?
                        .combine(&Region32::rectangle(bounds), 1)?
                        .complexity()
                        != 1,
                ))
            }
            GdiApi32::SelectClipRgn | GdiApi32::ExtSelectClipRgn => {
                if args[1] == 0 {
                    if api == GdiApi32::ExtSelectClipRgn && args[2] != 5 {
                        return Err(GdiError32::InvalidRequest);
                    }
                    return self.model.set_clip(args[0], None);
                }
                let region = self.region(args[1])?;
                let mode = if api == GdiApi32::SelectClipRgn {
                    5
                } else {
                    args[2]
                };
                let next = if mode == 5 {
                    region
                } else {
                    state
                        .clip
                        .clone()
                        .unwrap_or_else(|| Region32::rectangle(bounds))
                        .combine(&region, mode)?
                };
                self.model.set_clip(args[0], Some(next))
            }
            GdiApi32::IntersectClipRect | GdiApi32::ExcludeClipRect => {
                let a = state.device_point(Point32 {
                    x: args[1] as i32,
                    y: args[2] as i32,
                })?;
                let b = state.device_point(Point32 {
                    x: args[3] as i32,
                    y: args[4] as i32,
                })?;
                let next = state
                    .clip
                    .clone()
                    .unwrap_or_else(|| Region32::rectangle(bounds))
                    .combine(
                        &Region32::rectangle(Rect32 {
                            left: a.x.min(b.x),
                            top: a.y.min(b.y),
                            right: a.x.max(b.x),
                            bottom: a.y.max(b.y),
                        }),
                        if api == GdiApi32::IntersectClipRect {
                            1
                        } else {
                            4
                        },
                    )?;
                self.model.set_clip(args[0], Some(next))
            }
            _ => Err(GdiError32::InvalidRequest),
        }
    }
}
