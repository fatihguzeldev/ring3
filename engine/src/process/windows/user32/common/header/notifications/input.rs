use super::*;
impl User32Context32 {
    pub(in crate::process::windows::user32::common::header) fn header_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        if !self.windows().get(window)?.description.enabled {
            return Ok(User32Outcome32::Return(0));
        }
        let style = self.state.record(window)?.style;
        let point = Point32 {
            x: i32::from(args[3] as i16),
            y: i32::from((args[3] >> 16) as i16),
        };
        let extent = self.windows().get(window)?.description.extent;
        let (hit, flags) =
            self.header(window)?
                .hit(point, extent.width as i32, extent.height as i32);
        let current = self.header(window)?.track;
        let (index, action, stage, width) = match args[1] {
            0x201 | 0x203 if hit >= 0 => {
                let index = hit as usize;
                let item = &self.header(window)?.items[index];
                let divider = flags & 12 != 0;
                if !divider && style & 2 == 0 {
                    return Ok(User32Outcome32::Return(0));
                }
                if args[1] == 0x203 {
                    (
                        index,
                        Action32::Click {
                            double: true,
                            divider,
                        },
                        Stage32::Notify,
                        item.width,
                    )
                } else {
                    (
                        index,
                        Action32::Begin {
                            divider,
                            origin: point.x,
                            width: item.width,
                        },
                        if divider {
                            Stage32::Notify
                        } else {
                            Stage32::Capture
                        },
                        item.width,
                    )
                }
            }
            0x200
                if current.is_some_and(|track| track.divider)
                    && self.windows().capture() == Some(window) =>
            {
                let track = current.unwrap();
                let Some(index) = self
                    .header(window)?
                    .items
                    .iter()
                    .position(|item| item.key == track.key)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                let width = (i64::from(track.width) + i64::from(point.x) - i64::from(track.origin))
                    .clamp(0, i64::from(i32::MAX)) as i32;
                (
                    index,
                    Action32::Move {
                        full: style & 0x80 != 0,
                    },
                    Stage32::Notify,
                    width,
                )
            }
            0x202 if current.is_some() => {
                let track = current.unwrap();
                let Some(index) = self
                    .header(window)?
                    .items
                    .iter()
                    .position(|item| item.key == track.key)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                if track.divider {
                    (index, Action32::End, Stage32::Notify, track.proposed)
                } else if hit == index as i32 {
                    (
                        index,
                        Action32::Click {
                            double: false,
                            divider: false,
                        },
                        Stage32::Notify,
                        track.width,
                    )
                } else {
                    (index, Action32::Cancel, Stage32::Release, track.width)
                }
            }
            0x215 | 0x1f if current.is_some() => {
                let track = current.unwrap();
                let Some(index) = self
                    .header(window)?
                    .items
                    .iter()
                    .position(|item| item.key == track.key)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                (index, Action32::Cancel, Stage32::Release, track.width)
            }
            _ => return Ok(User32Outcome32::Return(0)),
        };
        let item = HeaderItem32 {
            key: 0,
            width,
            text: Vec::new(),
            format: 0,
            data: 0,
        };
        self.start_header_call(window, index, &item, 1, action, stage, ram, call)
    }
}
