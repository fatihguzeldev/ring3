use crate::windows::user32::{Point32, Rect32, User32Error32 as E};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ItemSize32 {
    pub width: i32,
    pub height: i32,
    pub kind: u32,
}

pub(crate) struct MenuLayout32 {
    pub items: Vec<Rect32>,
    pub extent: Point32,
}

impl MenuLayout32 {
    pub(crate) fn bar(items: &[ItemSize32], width: i32) -> Result<Self, E> {
        if width <= 0 || items.len() > 4096 {
            return Err(E::Win32(87));
        }
        let mut rects = Vec::new();
        rects
            .try_reserve_exact(items.len())
            .map_err(|_| E::Allocation)?;
        let (mut x, mut y, mut row_height, mut row_start) = (0i32, 0i32, 0i32, 0usize);
        for item in items {
            item.check()?;
            if x != 0
                && (item.kind & 0x60 != 0 || x.checked_add(item.width).ok_or(E::Capacity)? > width)
            {
                finish_bar_row(&mut rects[row_start..], width, row_height, items, row_start)?;
                y = y.checked_add(row_height).ok_or(E::Capacity)?;
                x = 0;
                row_height = 0;
                row_start = rects.len();
            }
            rects.push(Rect32 {
                left: x,
                top: y,
                right: x.checked_add(item.width).ok_or(E::Capacity)?.min(width),
                bottom: y.checked_add(item.height).ok_or(E::Capacity)?,
            });
            x = x.checked_add(item.width).ok_or(E::Capacity)?;
            row_height = row_height.max(item.height);
        }
        finish_bar_row(&mut rects[row_start..], width, row_height, items, row_start)?;
        Ok(Self {
            items: rects,
            extent: Point32 {
                x: width,
                y: y.checked_add(row_height).ok_or(E::Capacity)?,
            },
        })
    }

    pub(crate) fn popup(items: &[ItemSize32], border: i32) -> Result<Self, E> {
        if !(0..=16).contains(&border) || items.len() > 4096 {
            return Err(E::Win32(87));
        }
        let mut rects = Vec::new();
        rects
            .try_reserve_exact(items.len())
            .map_err(|_| E::Allocation)?;
        let (mut x, mut y, mut column_width, mut column_start) = (border, border, 0i32, 0usize);
        let mut height = border;
        for item in items {
            item.check()?;
            if rects.len() != column_start && item.kind & 0x60 != 0 {
                finish_popup_column(&mut rects[column_start..], column_width)?;
                x = x.checked_add(column_width).ok_or(E::Capacity)?;
                column_width = 0;
                column_start = rects.len();
                y = border;
            }
            rects.push(Rect32 {
                left: x,
                top: y,
                right: x.checked_add(item.width).ok_or(E::Capacity)?,
                bottom: y.checked_add(item.height).ok_or(E::Capacity)?,
            });
            column_width = column_width.max(item.width);
            y = y.checked_add(item.height).ok_or(E::Capacity)?;
            height = height.max(y);
        }
        finish_popup_column(&mut rects[column_start..], column_width)?;
        Ok(Self {
            items: rects,
            extent: Point32 {
                x: x.checked_add(column_width)
                    .and_then(|n| n.checked_add(border))
                    .ok_or(E::Capacity)?,
                y: height.checked_add(border).ok_or(E::Capacity)?,
            },
        })
    }

    pub(crate) fn item_at(&self, point: Point32) -> Option<usize> {
        self.items.iter().position(|rect| {
            point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom
        })
    }

    pub(crate) fn screen_item(&self, index: usize, origin: Point32) -> Result<Rect32, E> {
        let rect = self.items.get(index).copied().ok_or(E::Win32(1456))?;
        Ok(Rect32 {
            left: rect.left.checked_add(origin.x).ok_or(E::Capacity)?,
            top: rect.top.checked_add(origin.y).ok_or(E::Capacity)?,
            right: rect.right.checked_add(origin.x).ok_or(E::Capacity)?,
            bottom: rect.bottom.checked_add(origin.y).ok_or(E::Capacity)?,
        })
    }
}

impl ItemSize32 {
    fn check(self) -> Result<(), E> {
        if self.width <= 0 || self.height < 0 || self.height == 0 && self.kind & 0x100 == 0 {
            return Err(E::Win32(87));
        }
        Ok(())
    }
}

fn finish_bar_row(
    row: &mut [Rect32],
    width: i32,
    height: i32,
    items: &[ItemSize32],
    start: usize,
) -> Result<(), E> {
    let right_start = items[start..start + row.len()]
        .iter()
        .position(|item| item.kind & 0x4000 != 0);
    let shift = right_start
        .map(|_| width - row.last().unwrap().right)
        .unwrap_or(0);
    for (index, rect) in row.iter_mut().enumerate() {
        rect.bottom = rect.top.checked_add(height).ok_or(E::Capacity)?;
        if right_start.is_some_and(|start| index >= start) {
            rect.left = rect.left.checked_add(shift).ok_or(E::Capacity)?;
            rect.right = rect.right.checked_add(shift).ok_or(E::Capacity)?;
        }
    }
    Ok(())
}

fn finish_popup_column(column: &mut [Rect32], width: i32) -> Result<(), E> {
    for rect in column {
        rect.right = rect.left.checked_add(width).ok_or(E::Capacity)?;
    }
    Ok(())
}

pub(crate) fn popup_origin(
    anchor: Point32,
    extent: Point32,
    desktop: Rect32,
    flags: u32,
    exclude: Option<Rect32>,
) -> Result<Point32, E> {
    if extent.x <= 0
        || extent.y <= 0
        || desktop.left >= desktop.right
        || desktop.top >= desktop.bottom
        || flags & 0x0c == 0x0c
        || flags & 0x30 == 0x30
    {
        return Err(E::Win32(87));
    }
    let mut x = i64::from(anchor.x)
        - if flags & 8 != 0 {
            i64::from(extent.x)
        } else if flags & 4 != 0 {
            i64::from(extent.x / 2)
        } else {
            0
        };
    let mut y = i64::from(anchor.y)
        - if flags & 0x20 != 0 {
            i64::from(extent.y)
        } else if flags & 0x10 != 0 {
            i64::from(extent.y / 2)
        } else {
            0
        };
    let width = i64::from(extent.x);
    let height = i64::from(extent.y);
    if let Some(rect) = exclude {
        if rect.left > rect.right || rect.top > rect.bottom {
            return Err(E::Win32(87));
        }
        if x < i64::from(rect.right)
            && x + width > i64::from(rect.left)
            && y < i64::from(rect.bottom)
            && y + height > i64::from(rect.top)
        {
            if i64::from(rect.bottom) + height <= i64::from(desktop.bottom) {
                y = i64::from(rect.bottom);
            } else if i64::from(rect.top) - height >= i64::from(desktop.top) {
                y = i64::from(rect.top) - height;
            } else if i64::from(rect.right) + width <= i64::from(desktop.right) {
                x = i64::from(rect.right);
            } else {
                x = i64::from(rect.left) - width;
            }
        }
    }
    x = x
        .max(i64::from(desktop.left))
        .min((i64::from(desktop.right) - width).max(i64::from(desktop.left)));
    y = y
        .max(i64::from(desktop.top))
        .min((i64::from(desktop.bottom) - height).max(i64::from(desktop.top)));
    Ok(Point32 {
        x: x as i32,
        y: y as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(width: i32, height: i32, kind: u32) -> ItemSize32 {
        ItemSize32 {
            width,
            height,
            kind,
        }
    }
    #[test]
    fn bar_wraps_whole_items_and_right_justifies_only_the_tail_of_its_row() {
        let layout = MenuLayout32::bar(
            &[
                item(30, 10, 0),
                item(30, 12, 0),
                item(20, 8, 0x4000),
                item(40, 9, 0),
            ],
            100,
        )
        .unwrap();
        assert_eq!(layout.extent, Point32 { x: 100, y: 21 });
        assert_eq!(
            layout.items[0],
            Rect32 {
                left: 0,
                top: 0,
                right: 30,
                bottom: 12
            }
        );
        assert_eq!(
            layout.items[2],
            Rect32 {
                left: 80,
                top: 0,
                right: 100,
                bottom: 12
            }
        );
        assert_eq!(
            layout.items[3],
            Rect32 {
                left: 0,
                top: 12,
                right: 40,
                bottom: 21
            }
        );
        assert_eq!(layout.item_at(Point32 { x: 100, y: 2 }), None);
    }
    #[test]
    fn popup_columns_have_independent_widths_and_screen_coordinates_are_checked() {
        let layout = MenuLayout32::popup(
            &[
                item(30, 10, 0),
                item(50, 20, 0),
                item(20, 12, 0x40),
                item(25, 10, 0),
            ],
            2,
        )
        .unwrap();
        assert_eq!(layout.extent, Point32 { x: 79, y: 34 });
        assert_eq!(
            layout.items[0],
            Rect32 {
                left: 2,
                top: 2,
                right: 52,
                bottom: 12
            }
        );
        assert_eq!(
            layout.items[2],
            Rect32 {
                left: 52,
                top: 2,
                right: 77,
                bottom: 14
            }
        );
        assert_eq!(
            layout.screen_item(2, Point32 { x: -20, y: 100 }).unwrap(),
            Rect32 {
                left: 32,
                top: 102,
                right: 57,
                bottom: 114
            }
        );
        assert!(
            layout
                .screen_item(2, Point32 { x: i32::MAX, y: 0 })
                .is_err()
        );
    }
    #[test]
    fn popup_alignment_exclusion_and_virtual_desktop_edges_do_not_wrap() {
        let desktop = Rect32 {
            left: -100,
            top: -100,
            right: 100,
            bottom: 100,
        };
        assert_eq!(
            popup_origin(
                Point32 { x: 0, y: 0 },
                Point32 { x: 40, y: 20 },
                desktop,
                4 | 0x10,
                None
            )
            .unwrap(),
            Point32 { x: -20, y: -10 }
        );
        assert_eq!(
            popup_origin(
                Point32 { x: 95, y: 95 },
                Point32 { x: 40, y: 20 },
                desktop,
                0,
                None
            )
            .unwrap(),
            Point32 { x: 60, y: 80 }
        );
        assert_eq!(
            popup_origin(
                Point32::default(),
                Point32 { x: 40, y: 20 },
                desktop,
                0,
                Some(Rect32 {
                    left: -10,
                    top: -10,
                    right: 10,
                    bottom: 10
                })
            )
            .unwrap(),
            Point32 { x: 0, y: 10 }
        );
        assert!(
            popup_origin(
                Point32::default(),
                Point32 { x: 40, y: 20 },
                desktop,
                0x0c,
                None
            )
            .is_err()
        );
    }
}
