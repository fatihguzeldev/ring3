use crate::windows::{
    controls::ControlKind32,
    gdi::LogicalFont32,
    text::CodePage32,
    user32::{
        Point32, User32Error32 as E,
        dialogs::{DialogFont32, DialogName32, DialogTemplate32, DialogUnits32},
    },
};

#[derive(Clone, Copy)]
pub(super) enum Pointer32 {
    Value(u32),
    Offset(u32),
}

impl Pointer32 {
    fn address(self, base: u32) -> Result<u32, E> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Offset(offset) => base.checked_add(offset).ok_or(E::Win32(87)),
        }
    }
}

pub(super) struct Window32 {
    class: Pointer32,
    title: Pointer32,
    creation: Pointer32,
    style: u32,
    ex_style: u32,
    position: Point32,
    width: i32,
    height: i32,
    pub id: u32,
    pub help: u32,
    pub parent: Option<usize>,
    pub font: u32,
    pub dialog_base: Option<Point32>,
}

impl Window32 {
    pub(super) fn arguments(&self, base: u32, instance: u32, parent: u32) -> Result<[u32; 12], E> {
        Ok([
            self.ex_style,
            self.class.address(base)?,
            self.title.address(base)?,
            self.style,
            self.position.x as u32,
            self.position.y as u32,
            self.width as u32,
            self.height as u32,
            parent,
            self.id,
            instance,
            self.creation.address(base)?,
        ])
    }
}

pub(super) struct Prepared32 {
    pub bytes: Vec<u8>,
    pub windows: Vec<Window32>,
    pub base: Point32,
    pub visible: bool,
}

impl Prepared32 {
    pub(super) fn new(
        template: DialogTemplate32,
        base: Point32,
        wide: bool,
        ansi: CodePage32,
        byte_limit: usize,
    ) -> Result<Self, E> {
        if base.x <= 0 || base.y <= 0 || !matches!(template.menu, DialogName32::Empty) {
            return Err(E::Win32(if base.x <= 0 || base.y <= 0 { 87 } else { 50 }));
        }
        let mut output = Output32 {
            bytes: Vec::new(),
            wide,
            ansi,
            limit: byte_limit,
        };
        // offset zero remains distinct from a null optional creation pointer.
        output.append(&[0; 32])?;
        let mut windows = Vec::new();
        windows
            .try_reserve_exact(template.items.len() + 1)
            .map_err(|_| E::Allocation)?;
        let visible = template.style & 0x1000_0000 != 0;
        let root_class = if matches!(template.class, DialogName32::Empty) {
            DialogName32::Text("#32770".encode_utf16().collect())
        } else {
            template.class
        };
        let (position, width, height) = pixels(template.units, base)?;
        windows.push(Window32 {
            class: output.name(&root_class, false)?,
            title: output.text(&template.title)?,
            creation: Pointer32::Value(0),
            // the dialog is shown only after all child windows and WM_INITDIALOG.
            style: template.style & !0x1000_0000,
            ex_style: template.ex_style,
            position,
            width,
            height,
            id: 0,
            help: template.help,
            parent: None,
            font: 0,
            dialog_base: Some(base),
        });
        for item in template.items {
            let (position, width, height) = pixels(item.units, base)?;
            let creation = if item.creation.is_empty() {
                Pointer32::Value(0)
            } else {
                output.align(2)?;
                Pointer32::Offset(output.append(&item.creation)?)
            };
            windows.push(Window32 {
                class: output.name(&item.class, true)?,
                title: output.name(&item.title, false)?,
                creation,
                style: item.style | 0x4000_0000,
                ex_style: item.ex_style,
                position,
                width,
                height,
                id: item.id,
                help: item.help,
                parent: Some(0),
                font: 0,
                dialog_base: None,
            });
        }
        Ok(Self {
            bytes: output.bytes,
            windows,
            base,
            visible,
        })
    }
    pub(super) fn set_font(&mut self, font: u32) {
        for window in &mut self.windows {
            window.font = font;
        }
    }
    pub(super) fn append_child(&mut self, mut child: Self, id: u32, limit: usize) -> Result<(), E> {
        let offset = u32::try_from(self.bytes.len()).map_err(|_| E::Capacity)?;
        let length = self
            .bytes
            .len()
            .checked_add(child.bytes.len())
            .filter(|length| *length <= limit)
            .ok_or(E::Capacity)?;
        let first = self.windows.len();
        self.bytes
            .try_reserve_exact(length - self.bytes.len())
            .map_err(|_| E::Allocation)?;
        self.windows
            .try_reserve_exact(child.windows.len())
            .map_err(|_| E::Allocation)?;
        for window in &mut child.windows {
            for pointer in [&mut window.class, &mut window.title, &mut window.creation] {
                if let Pointer32::Offset(value) = pointer {
                    *value = value.checked_add(offset).ok_or(E::Capacity)?;
                }
            }
            window.parent = Some(window.parent.map_or(0, |index| first + index));
        }
        let custom = &mut child.windows[0];
        custom.id = id;
        custom.style |= 0x5000_0000;
        custom.position = Point32 {
            x: 0,
            y: self.windows[0].height,
        };
        let height = self.windows[0]
            .height
            .checked_add(custom.height)
            .ok_or(E::Capacity)?;
        self.windows[0].width = self.windows[0].width.max(custom.width);
        self.windows[0].height = height;
        self.bytes.append(&mut child.bytes);
        self.windows.append(&mut child.windows);
        Ok(())
    }
}

pub(super) fn logical_font(input: &DialogFont32, dpi: Point32) -> Result<LogicalFont32, E> {
    let height = mul_div(i32::from(input.points), dpi.y, 72)?;
    let font = LogicalFont32 {
        height: height.checked_neg().ok_or(E::Win32(87))?,
        // native dialog creation uses regular weight for both template layouts.
        weight: 400,
        italic: input.italic,
        charset: if input.charset == 1 { 0 } else { input.charset },
        face: input.face.clone(),
        ..Default::default()
    };
    font.check().map_err(|_| E::Win32(87))?;
    Ok(font)
}

pub(super) fn mul_div(value: i32, numerator: i32, denominator: i32) -> Result<i32, E> {
    if denominator <= 0 {
        return Err(E::Win32(87));
    }
    let product = i64::from(value) * i64::from(numerator);
    let half = i64::from(denominator) / 2;
    let rounded = if product < 0 {
        product - half
    } else {
        product + half
    };
    i32::try_from(rounded / i64::from(denominator)).map_err(|_| E::Win32(87))
}

fn pixels(units: DialogUnits32, base: Point32) -> Result<(Point32, i32, i32), E> {
    let width = mul_div(i32::from(units.width), base.x, 4)?;
    let height = mul_div(i32::from(units.height), base.y, 8)?;
    if width < 0 || height < 0 {
        return Err(E::Win32(87));
    }
    Ok((
        Point32 {
            x: mul_div(i32::from(units.x), base.x, 4)?,
            y: mul_div(i32::from(units.y), base.y, 8)?,
        },
        width,
        height,
    ))
}

struct Output32 {
    bytes: Vec<u8>,
    wide: bool,
    ansi: CodePage32,
    limit: usize,
}

impl Output32 {
    fn append(&mut self, bytes: &[u8]) -> Result<u32, E> {
        let offset = self.bytes.len();
        let end = offset
            .checked_add(bytes.len())
            .filter(|&end| end <= self.limit)
            .ok_or(E::Capacity)?;
        self.bytes
            .try_reserve_exact(end - offset)
            .map_err(|_| E::Allocation)?;
        self.bytes.extend_from_slice(bytes);
        u32::try_from(offset).map_err(|_| E::Capacity)
    }

    fn align(&mut self, alignment: usize) -> Result<(), E> {
        let count = self.bytes.len().next_multiple_of(alignment) - self.bytes.len();
        self.append(&[0; 2][..count])?;
        Ok(())
    }

    fn text(&mut self, units: &[u16]) -> Result<Pointer32, E> {
        if self.wide {
            self.align(2)?;
            let offset = self.bytes.len();
            for unit in units.iter().copied().chain(std::iter::once(0)) {
                self.append(&unit.to_le_bytes())?;
            }
            Ok(Pointer32::Offset(offset as u32))
        } else {
            let encoded = self.ansi.encode(units, false).map_err(|_| E::Win32(1113))?;
            let offset = self.append(&encoded.0)?;
            self.append(&[0])?;
            Ok(Pointer32::Offset(offset))
        }
    }

    fn name(&mut self, name: &DialogName32, control: bool) -> Result<Pointer32, E> {
        match name {
            DialogName32::Empty => Ok(Pointer32::Value(0)),
            DialogName32::Ordinal(ordinal) if control && (0x80..=0x85).contains(ordinal) => {
                let kind = ControlKind32::from_resource_ordinal(*ordinal).ok_or(E::Win32(1407))?;
                self.text(&kind.name().encode_utf16().collect::<Vec<_>>())
            }
            DialogName32::Ordinal(value) => Ok(Pointer32::Value(u32::from(*value))),
            DialogName32::Text(units) => self.text(units),
        }
    }
}

#[cfg(test)]
mod tests;
