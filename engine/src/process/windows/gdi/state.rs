use super::{context::GdiRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::Point32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process) fn display_dpi(&self) -> Point32 {
        Point32 { x: 96, y: 96 }
    }
    pub(super) fn scalar_state(&mut self, api: GdiApi32, args: &[u32]) -> Result<u32, GdiError32> {
        let mut state = self.model.state(args[0])?.clone();
        let setting = matches!(
            api,
            GdiApi32::SetTextColor
                | GdiApi32::SetBkColor
                | GdiApi32::SetBkMode
                | GdiApi32::SetDCBrushColor
                | GdiApi32::SetDCPenColor
                | GdiApi32::SetROP2
                | GdiApi32::SetStretchBltMode
                | GdiApi32::SetMapMode
        );
        if setting
            && ((api == GdiApi32::SetBkMode && !(1..=2).contains(&args[1]))
                || (api == GdiApi32::SetROP2 && !(1..=16).contains(&args[1]))
                || (api == GdiApi32::SetStretchBltMode && !matches!(args[1], 1 | 2 | 3 | 4))
                || (api == GdiApi32::SetMapMode && !(1..=8).contains(&args[1])))
        {
            return Err(GdiError32::InvalidRequest);
        }
        let slot = match api {
            GdiApi32::SetTextColor | GdiApi32::GetTextColor => &mut state.text_color,
            GdiApi32::SetBkColor | GdiApi32::GetBkColor => &mut state.background_color,
            GdiApi32::SetBkMode | GdiApi32::GetBkMode => &mut state.background_mode,
            GdiApi32::SetDCBrushColor | GdiApi32::GetDCBrushColor => &mut state.dc_brush_color,
            GdiApi32::SetDCPenColor | GdiApi32::GetDCPenColor => &mut state.dc_pen_color,
            GdiApi32::SetROP2 | GdiApi32::GetROP2 => &mut state.rop2,
            GdiApi32::SetStretchBltMode | GdiApi32::GetStretchBltMode => &mut state.stretch_mode,
            GdiApi32::SetMapMode | GdiApi32::GetMapMode => &mut state.map_mode,
            _ => return Err(GdiError32::InvalidRequest),
        };
        let previous = *slot;
        if setting {
            *slot = args[1];
            if api == GdiApi32::SetMapMode {
                let (logical, device) = match args[1] {
                    1 => (1, 1),
                    2 => (254, -96),
                    3 => (2540, -96),
                    4 => (100, -96),
                    5 => (1000, -96),
                    6 => (1440, -96),
                    _ => (state.window_extent.x, state.viewport_extent.y),
                };
                if args[1] <= 6 {
                    state.window_extent = Point32 {
                        x: logical,
                        y: logical,
                    };
                    state.viewport_extent = Point32 {
                        x: device.abs(),
                        y: device,
                    };
                }
            }
            self.model.set_state(args[0], state)?;
        }
        Ok(previous)
    }
    pub(super) fn point_state(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let mut state = self.model.state(args[0])?.clone();
        let getter = matches!(
            api,
            GdiApi32::GetWindowOrgEx
                | GdiApi32::GetViewportOrgEx
                | GdiApi32::GetWindowExtEx
                | GdiApi32::GetViewportExtEx
                | GdiApi32::GetBrushOrgEx
        );
        let extent = matches!(api, GdiApi32::SetWindowExtEx | GdiApi32::SetViewportExtEx);
        let variable = matches!(state.map_mode, 7 | 8);
        let slot = match api {
            GdiApi32::SetWindowOrgEx | GdiApi32::GetWindowOrgEx | GdiApi32::OffsetWindowOrgEx => {
                &mut state.window_origin
            }
            GdiApi32::SetViewportOrgEx
            | GdiApi32::GetViewportOrgEx
            | GdiApi32::OffsetViewportOrgEx => &mut state.viewport_origin,
            GdiApi32::SetWindowExtEx | GdiApi32::GetWindowExtEx => &mut state.window_extent,
            GdiApi32::SetViewportExtEx | GdiApi32::GetViewportExtEx => &mut state.viewport_extent,
            GdiApi32::SetBrushOrgEx | GdiApi32::GetBrushOrgEx => &mut state.brush_origin,
            GdiApi32::MoveToEx => &mut state.current_position,
            _ => return Err(GdiError32::InvalidRequest),
        };
        let previous = *slot;
        if !getter && (!extent || variable) {
            let offset = matches!(
                api,
                GdiApi32::OffsetWindowOrgEx | GdiApi32::OffsetViewportOrgEx
            );
            let next = Point32 {
                x: if offset {
                    slot.x
                        .checked_add(args[1] as i32)
                        .ok_or(GdiError32::InvalidRequest)?
                } else {
                    args[1] as i32
                },
                y: if offset {
                    slot.y
                        .checked_add(args[2] as i32)
                        .ok_or(GdiError32::InvalidRequest)?
                } else {
                    args[2] as i32
                },
            };
            if extent && (next.x == 0 || next.y == 0) {
                return Err(GdiError32::InvalidRequest);
            }
            *slot = next;
        }
        if !getter && extent && state.map_mode == 7 {
            normalize_isotropic(&mut state)?;
        }
        let output = if getter { args[1] } else { args[3] };
        if getter || output != 0 {
            memory::write_point(memory, output, previous)?;
        }
        if !getter {
            self.model.set_state(args[0], state)?;
        }
        Ok(1)
    }
    pub(super) fn transform_points(
        &self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if args[2] > 65_536 {
            return Err(GdiError32::Capacity);
        }
        let state = self.model.state(args[0])?;
        let mut bytes = memory::zeroes(args[2] as usize * 8)?;
        memory.read(GuestAddress(args[1]), &mut bytes)?;
        for point in bytes.chunks_exact_mut(8) {
            let input = Point32 {
                x: i32::from_le_bytes(point[..4].try_into().unwrap()),
                y: i32::from_le_bytes(point[4..].try_into().unwrap()),
            };
            let output = if api == GdiApi32::DPtoLP {
                state.logical_point(input)?
            } else {
                state.device_point(input)?
            };
            point[..4].copy_from_slice(&output.x.to_le_bytes());
            point[4..].copy_from_slice(&output.y.to_le_bytes());
        }
        memory.write(GuestAddress(args[1]), &bytes)?;
        Ok(1)
    }
    pub(super) fn device_caps(&self, dc: u32, index: u32) -> Result<u32, GdiError32> {
        self.model.dc_kind(dc)?;
        Ok(match index {
            0 => 0x400,
            2 => 1,
            4 => self.config.screen_width * 254 / 960,
            6 => self.config.screen_height * 254 / 960,
            8 | 118 => self.config.screen_width,
            10 | 117 => self.config.screen_height,
            12 => 32,
            14 => 1,
            16 | 18 | 24 => u32::MAX,
            20 | 22 | 26 | 28 | 104 | 106 => 0,
            30 => 2 | 16,
            32 => 2 | 16 | 128,
            34 => 32 | 64 | 128 | 256 | 1024 | 2048 | 4096 | 8192,
            36 => 1,
            38 => 1 | 8 | 128 | 512 | 2048 | 8192,
            40 | 42 => 36,
            44 => 51,
            88 => self.display_dpi().x as u32,
            90 => self.display_dpi().y as u32,
            108 => 24,
            116 => 60,
            _ => 0,
        })
    }
}

fn normalize_isotropic(state: &mut crate::windows::gdi::DcState32) -> Result<(), GdiError32> {
    let wx = i64::from(state.window_extent.x).abs();
    let wy = i64::from(state.window_extent.y).abs();
    let vx = i64::from(state.viewport_extent.x).abs();
    let vy = i64::from(state.viewport_extent.y).abs();
    if wx == 0 || wy == 0 || vx == 0 || vy == 0 {
        return Err(GdiError32::InvalidRequest);
    }
    let scaled =
        |value: i64, numerator: i64, denominator: i64, sign: i32| -> Result<i32, GdiError32> {
            let value = ((value * numerator + denominator / 2) / denominator).max(1);
            i32::try_from(value * i64::from(sign.signum())).map_err(|_| GdiError32::InvalidRequest)
        };
    if vx * wy > vy * wx {
        state.viewport_extent.x = scaled(vy, wx, wy, state.viewport_extent.x)?;
    } else {
        state.viewport_extent.y = scaled(vx, wy, wx, state.viewport_extent.y)?;
    }
    Ok(())
}
