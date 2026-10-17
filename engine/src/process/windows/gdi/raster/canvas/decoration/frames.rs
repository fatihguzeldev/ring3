use super::*;
use crate::{memory::AddressSpace, windows::gdi::DcKind32};

impl GdiCanvas32 {
    pub(in crate::process::windows::gdi) fn frame_control(
        &mut self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        rect: Rect32,
        kind: u32,
        flags: u32,
    ) -> Result<Rect32, GdiError32> {
        let shape = flags & 255;
        if flags & !0xffff != 0 || !(1..=5).contains(&kind) {
            return Err(GdiError32::InvalidRequest);
        }
        let valid = match kind {
            1 => shape <= 4,
            2 | 5 => matches!(shape, 0 | 1 | 2 | 4 | 0x20 | 0x40),
            3 => matches!(shape, 0 | 1 | 2 | 3 | 5 | 8 | 16),
            4 => matches!(shape, 0 | 1 | 2 | 4 | 8 | 16),
            _ => false,
        };
        if !valid
            || flags & 0x800 != 0 && (!(kind == 2 || kind == 5) || !matches!(shape, 0x20 | 0x40))
            || flags & 0x2000 != 0 && (kind != 4 || shape != 16)
        {
            return Err(GdiError32::InvalidRequest);
        }
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return Ok(rect);
        }
        let mask = kind == 2 || kind == 5 || kind == 4 && shape != 16;
        if mask && !matches!(self.owner.dc_kind, DcKind32::Memory { .. }) {
            return Err(GdiError32::InvalidRequest);
        }
        let ink = if mask {
            0
        } else {
            gdi.system_color(if flags & 0x100 != 0 { 17 } else { 18 })
        };
        let paper = if mask { 0xffffff } else { gdi.system_color(15) };
        if flags & 0x800 == 0 {
            self.fill(gdi, rect, if kind == 4 && shape == 2 { 0 } else { paper });
        }
        let mut content = shrink(rect, 2);
        let pushed = flags & (0x200 | 0x400) != 0;
        if kind == 1 || kind == 3 && shape < 8 || kind == 4 && matches!(shape, 0 | 8 | 16) {
            let border = if pushed { 10 } else { 5 };
            if mask {
                self.frame(gdi, rect, ink, ink);
            } else {
                self.edge(gdi, rect, border, 15 | (flags & 0xc000))?;
            }
        }
        if kind == 4 && shape == 16 {
            return Ok(content);
        }
        if flags & 0x200 != 0 {
            content = Rect32 {
                left: content.left.saturating_add(1),
                top: content.top.saturating_add(1),
                right: content.right.saturating_add(1),
                bottom: content.bottom.saturating_add(1),
            };
        }
        let cx = ((i64::from(content.left) + i64::from(content.right)) / 2) as i32;
        let cy = ((i64::from(content.top) + i64::from(content.bottom)) / 2) as i32;
        match kind {
            1 => match shape {
                0 => {
                    self.decoration_line(
                        gdi,
                        Point32 {
                            x: content.left,
                            y: content.top,
                        },
                        Point32 {
                            x: content.right.saturating_sub(1),
                            y: content.bottom.saturating_sub(1),
                        },
                        ink,
                    )?;
                    self.decoration_line(
                        gdi,
                        Point32 {
                            x: content.right.saturating_sub(1),
                            y: content.top,
                        },
                        Point32 {
                            x: content.left,
                            y: content.bottom.saturating_sub(1),
                        },
                        ink,
                    )?;
                }
                1 => self.fill(
                    gdi,
                    Rect32 {
                        top: content.bottom.saturating_sub(2),
                        ..content
                    },
                    ink,
                ),
                2 => {
                    self.frame(gdi, content, ink, ink);
                    self.fill(
                        gdi,
                        Rect32 {
                            bottom: content.top.saturating_add(2),
                            ..content
                        },
                        ink,
                    );
                }
                3 => {
                    let small = Rect32 {
                        left: cx,
                        top: content.top,
                        right: content.right,
                        bottom: cy,
                    };
                    self.frame(gdi, small, ink, ink);
                    self.frame(
                        gdi,
                        Rect32 {
                            left: content.left,
                            top: cy.saturating_sub(1),
                            right: cx.saturating_add(1),
                            bottom: content.bottom,
                        },
                        ink,
                        ink,
                    );
                }
                4 => self.text(
                    gdi,
                    memory,
                    super::super::CanvasText32 {
                        units: &[b'?' as u16],
                        bounds: content,
                        color: ink,
                        background: None,
                        horizontal: 1,
                        middle: true,
                        mnemonic: false,
                        multiline: false,
                    },
                )?,
                _ => unreachable!(),
            },
            2 | 5 => match shape {
                0 | 4 | 0x20 | 0x40 => self.decoration_arrow(
                    gdi,
                    content,
                    match shape {
                        4 => 2,
                        0x20 => 0,
                        0x40 => 1,
                        _ => 3,
                    },
                    ink,
                ),
                1 => self.decoration_check(gdi, content, ink)?,
                2 => self.decoration_ellipse(gdi, shrink(content, 1), ink),
                _ => unreachable!(),
            },
            3 => {
                if shape < 8 {
                    self.decoration_arrow(gdi, content, if shape == 5 { 1 } else { shape }, ink)
                } else {
                    for step in [3i32, 7, 11] {
                        let (a, b) = if shape == 8 {
                            (
                                Point32 {
                                    x: rect.right.saturating_sub(step),
                                    y: rect.bottom.saturating_sub(1),
                                },
                                Point32 {
                                    x: rect.right.saturating_sub(1),
                                    y: rect.bottom.saturating_sub(step),
                                },
                            )
                        } else {
                            (
                                Point32 {
                                    x: rect.left,
                                    y: rect.bottom.saturating_sub(step),
                                },
                                Point32 {
                                    x: rect.left.saturating_add(step).saturating_sub(1),
                                    y: rect.bottom.saturating_sub(1),
                                },
                            )
                        };
                        if a.x >= rect.left
                            && b.x < rect.right
                            && a.y >= rect.top
                            && b.y >= rect.top
                        {
                            self.decoration_line(gdi, a, b, ink)?;
                        }
                    }
                }
            }
            4 => match shape {
                0 => {
                    if flags & 0x400 != 0 {
                        self.decoration_check(gdi, content, ink)?;
                    }
                }
                8 => {
                    if flags & 0x400 != 0 {
                        let area = super::super::intersect(
                            content,
                            crate::process::windows::gdi::bitmaps::bounds(self.owner.layout),
                        );
                        for y in area.top..area.bottom {
                            for x in area.left..area.right {
                                if (x ^ y) & 1 == 0 {
                                    self.fill(
                                        gdi,
                                        Rect32 {
                                            left: x,
                                            top: y,
                                            right: x + 1,
                                            bottom: y + 1,
                                        },
                                        ink,
                                    );
                                }
                            }
                        }
                    }
                }
                1 | 2 | 4 => {
                    self.decoration_ellipse(gdi, rect, if shape == 2 { 0xffffff } else { ink });
                    if shape != 2 {
                        self.decoration_ellipse(gdi, shrink(rect, 1), paper);
                        if flags & 0x400 != 0 {
                            self.decoration_ellipse(gdi, shrink(rect, 3), ink);
                        }
                    }
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
        Ok(rect)
    }
    fn decoration_arrow(&mut self, gdi: &GdiRuntime32, rect: Rect32, direction: u32, color: u32) {
        let cx = ((i64::from(rect.left) + i64::from(rect.right)) / 2) as i32;
        let cy = ((i64::from(rect.top) + i64::from(rect.bottom)) / 2) as i32;
        let right = rect.right.saturating_sub(1);
        let bottom = rect.bottom.saturating_sub(1);
        let points = match direction {
            0 => [
                Point32 { x: cx, y: rect.top },
                Point32 {
                    x: rect.left,
                    y: bottom,
                },
                Point32 {
                    x: right,
                    y: bottom,
                },
            ],
            1 => [
                Point32 {
                    x: rect.left,
                    y: rect.top,
                },
                Point32 {
                    x: right,
                    y: rect.top,
                },
                Point32 { x: cx, y: bottom },
            ],
            2 => [
                Point32 {
                    x: rect.left,
                    y: cy,
                },
                Point32 {
                    x: right,
                    y: rect.top,
                },
                Point32 {
                    x: right,
                    y: bottom,
                },
            ],
            _ => [
                Point32 {
                    x: rect.left,
                    y: rect.top,
                },
                Point32 { x: right, y: cy },
                Point32 {
                    x: rect.left,
                    y: bottom,
                },
            ],
        };
        if rect.left < rect.right && rect.top < rect.bottom {
            self.decoration_triangle(gdi, points, color);
        }
    }
    fn decoration_check(
        &mut self,
        gdi: &GdiRuntime32,
        rect: Rect32,
        color: u32,
    ) -> Result<(), GdiError32> {
        let cy = ((i64::from(rect.top) + i64::from(rect.bottom)) / 2) as i32;
        let cx = ((i64::from(rect.left) * 2 + i64::from(rect.right)) / 3) as i32;
        self.decoration_line(
            gdi,
            Point32 {
                x: rect.left,
                y: cy,
            },
            Point32 {
                x: cx,
                y: rect.bottom.saturating_sub(1),
            },
            color,
        )?;
        self.decoration_line(
            gdi,
            Point32 {
                x: cx,
                y: rect.bottom.saturating_sub(1),
            },
            Point32 {
                x: rect.right.saturating_sub(1),
                y: rect.top,
            },
            color,
        )
    }
}
fn shrink(rect: Rect32, amount: i32) -> Rect32 {
    let left = rect.left.saturating_add(amount).min(rect.right);
    let top = rect.top.saturating_add(amount).min(rect.bottom);
    Rect32 {
        left,
        top,
        right: rect.right.saturating_sub(amount).max(left),
        bottom: rect.bottom.saturating_sub(amount).max(top),
    }
}
