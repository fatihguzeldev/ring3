use super::*;
use crate::windows::gdi::GdiContext32;
impl User32Context32 {
    pub(in crate::process::windows::user32::common) fn paint_numeric(
        &mut self,
        window: WindowHandle32,
        external: u32,
        message: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let erase = message == 0x14;
        let print = message == 0x318;
        if (erase || print) && external == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: projection.width as i32,
            bottom: projection.height as i32,
        };
        let region = if external != 0 {
            gdi.check_control_erase_dc(external)
                .map_err(HostError::Gdi)?;
            Region32::rectangle(bounds)
        } else {
            let region = self
                .state
                .update_region(window)
                .map_err(HostError::User32)?
                .0;
            if region.complexity() == 1 {
                return Ok(User32Outcome32::Return(0));
            }
            self.state.check_paint_change().map_err(HostError::User32)?;
            region
        };
        let font = match self.state.common_state(window).map_err(HostError::User32)? {
            State::Trackbar(value) => value.font,
            State::UpDown(value) => value.font,
            _ => return Err(HostError::InvalidRequest),
        };
        let font = if font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            font
        };
        let dc = if external != 0 {
            external
        } else {
            gdi.begin_window_paint_dc(projection, region, font, gdi.system_color(15))
                .map_err(HostError::Gdi)?
        };
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            canvas.fill(
                gdi,
                canvas.device_rect(bounds).map_err(HostError::Gdi)?,
                gdi.system_color(15),
            );
            if !erase {
                let style = self.state.record(window).map_err(HostError::User32)?.style;
                let enabled = self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .enabled;
                match self.state.common_state(window).map_err(HostError::User32)? {
                    State::Trackbar(value) => {
                        let vertical = style & 2 != 0;
                        let channel = value.channel(bounds.right, bounds.bottom, vertical);
                        canvas.fill(
                            gdi,
                            canvas.device_rect(channel).map_err(HostError::Gdi)?,
                            gdi.system_color(16),
                        );
                        canvas.frame(
                            gdi,
                            canvas.device_rect(channel).map_err(HostError::Gdi)?,
                            gdi.system_color(16),
                            gdi.system_color(20),
                        );
                        if style & 0x20 != 0 && value.selection.0 < value.selection.1 {
                            let a = value.coordinate(value.selection.0, channel, vertical);
                            let b = value.coordinate(value.selection.1, channel, vertical);
                            let selected = if vertical {
                                Rect32 {
                                    top: a,
                                    bottom: b,
                                    left: channel.left + 1,
                                    right: channel.right - 1,
                                }
                            } else {
                                Rect32 {
                                    left: a,
                                    right: b,
                                    top: channel.top + 1,
                                    bottom: channel.bottom - 1,
                                }
                            };
                            canvas.fill(
                                gdi,
                                canvas.device_rect(selected).map_err(HostError::Gdi)?,
                                gdi.system_color(13),
                            );
                        }
                        if style & 0x10 == 0 {
                            for index in 0..value.tick_count() + 2 {
                                let position = if index == 0 {
                                    value.minimum
                                } else if index == 1 {
                                    value.maximum
                                } else {
                                    value.tick(index - 2).unwrap()
                                };
                                let axis = value.coordinate(position, channel, vertical);
                                let sides = if style & 8 != 0 {
                                    [true, true]
                                } else {
                                    [style & 4 != 0, style & 4 == 0]
                                };
                                for (side, show) in sides.into_iter().enumerate() {
                                    if !show {
                                        continue;
                                    }
                                    let cross = if side == 0 {
                                        if vertical {
                                            channel.left - 6
                                        } else {
                                            channel.top - 6
                                        }
                                    } else if vertical {
                                        channel.right + 3
                                    } else {
                                        channel.bottom + 3
                                    };
                                    let tick = if vertical {
                                        Rect32 {
                                            left: cross,
                                            top: axis,
                                            right: cross + 4,
                                            bottom: axis + 1,
                                        }
                                    } else {
                                        Rect32 {
                                            left: axis,
                                            top: cross,
                                            right: axis + 1,
                                            bottom: cross + 4,
                                        }
                                    };
                                    canvas.fill(
                                        gdi,
                                        canvas.device_rect(tick).map_err(HostError::Gdi)?,
                                        gdi.system_color(18),
                                    );
                                }
                            }
                        }
                        if style & 0x80 == 0 {
                            let thumb = value.thumb(bounds.right, bounds.bottom, vertical);
                            canvas.fill(
                                gdi,
                                canvas.device_rect(thumb).map_err(HostError::Gdi)?,
                                gdi.system_color(if enabled { 15 } else { 17 }),
                            );
                            canvas.frame(
                                gdi,
                                canvas.device_rect(thumb).map_err(HostError::Gdi)?,
                                gdi.system_color(20),
                                gdi.system_color(16),
                            );
                        }
                        if self.windows().focus() == Some(window) {
                            canvas.focus(
                                gdi,
                                canvas
                                    .device_rect(Rect32 {
                                        left: 1,
                                        top: 1,
                                        right: bounds.right - 1,
                                        bottom: bounds.bottom - 1,
                                    })
                                    .map_err(HostError::Gdi)?,
                                gdi.system_color(18),
                            );
                        }
                    }
                    State::UpDown(value) => {
                        let horizontal = style & 0x40 != 0;
                        for side in 0..2 {
                            let rect = if horizontal {
                                Rect32 {
                                    left: bounds.right * side / 2,
                                    top: 0,
                                    right: bounds.right * (side + 1) / 2,
                                    bottom: bounds.bottom,
                                }
                            } else {
                                Rect32 {
                                    left: 0,
                                    top: bounds.bottom * side / 2,
                                    right: bounds.right,
                                    bottom: bounds.bottom * (side + 1) / 2,
                                }
                            };
                            let direction = if side == 0 {
                                if horizontal { -1 } else { 1 }
                            } else if horizontal {
                                1
                            } else {
                                -1
                            };
                            let pressed =
                                value.pressed.is_some_and(|(delta, _)| delta == direction);
                            canvas.frame(
                                gdi,
                                canvas.device_rect(rect).map_err(HostError::Gdi)?,
                                gdi.system_color(if pressed { 16 } else { 20 }),
                                gdi.system_color(if pressed { 20 } else { 16 }),
                            );
                            let center = Point32 {
                                x: (rect.left + rect.right) / 2,
                                y: (rect.top + rect.bottom) / 2,
                            };
                            for row in 0..4 {
                                let half = if side == 0 { row } else { 3 - row };
                                let line = if horizontal {
                                    Rect32 {
                                        left: center.x - 2 + row,
                                        top: center.y - half,
                                        right: center.x - 1 + row,
                                        bottom: center.y + half + 1,
                                    }
                                } else {
                                    Rect32 {
                                        left: center.x - half,
                                        top: center.y - 2 + row,
                                        right: center.x + half + 1,
                                        bottom: center.y - 1 + row,
                                    }
                                };
                                canvas.fill(
                                    gdi,
                                    canvas.device_rect(line).map_err(HostError::Gdi)?,
                                    gdi.system_color(if enabled { 18 } else { 17 }),
                                );
                            }
                        }
                    }
                    _ => return Err(HostError::InvalidRequest),
                }
            }
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("numeric paint owns its acquired dc")
            );
        }
        result?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("numeric paint was preflighted");
        }
        Ok(User32Outcome32::Return(u32::from(erase)))
    }
}
