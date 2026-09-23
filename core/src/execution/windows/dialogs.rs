use super::super::Access;
use super::{
    DispatchError, GuestMemory, MemoryError, Process32, Register32, callbacks, desktop, guest,
};

const MAX_TEMPLATE: usize = 65536;
const MAX_ITEMS: usize = 256;
const MAX_TEXT: usize = 1024;
const HOOK_SCRATCH: u32 = 64;

#[derive(Clone, Copy)]
enum Phase {
    Cbt,
    Init,
}

#[derive(Clone, Copy)]
pub(super) struct Pending {
    window: u32,
    procedure: u32,
    focus: u32,
    init: u32,
    phase: Phase,
}

pub(super) struct Template {
    pub(super) style: u32,
    pub(super) exstyle: u32,
    pub(super) title: String,
    pub(super) units: [i16; 4],
    pub(super) items: Vec<Item>,
}

pub(super) struct Item {
    pub(super) class: u16,
    pub(super) id: u32,
    pub(super) style: u32,
    pub(super) exstyle: u32,
    pub(super) title: Field,
    pub(super) units: [i16; 4],
}

pub(super) enum Field {
    Empty,
    Ordinal(u16),
    Text(String),
}

pub(super) fn parse(
    memory: &GuestMemory,
    pointer: u32,
    available: usize,
) -> Result<Template, DispatchError> {
    let mut reader = Reader {
        memory,
        pointer,
        available: available.min(MAX_TEMPLATE),
        offset: 0,
    };
    if reader.word()? != 1 || reader.word()? != 0xffff {
        return Err(DispatchError::Unsupported);
    }
    if reader.dword()? != 0 {
        return Err(DispatchError::Unsupported);
    }
    let exstyle = reader.dword()?;
    let style = reader.dword()?;
    let count = usize::from(reader.word()?);
    if count > MAX_ITEMS {
        return Err(DispatchError::Unsupported);
    }
    let units = [
        reader.word()?.cast_signed(),
        reader.word()?.cast_signed(),
        reader.word()?.cast_signed(),
        reader.word()?.cast_signed(),
    ];
    if !matches!(reader.field()?, Field::Empty) || !matches!(reader.field()?, Field::Empty) {
        return Err(DispatchError::Unsupported);
    }
    let Field::Text(title) = reader.field()? else {
        return Err(DispatchError::Unsupported);
    };
    if style & 0x40 != 0 {
        reader.word()?;
        reader.word()?;
        reader.word()?;
        if !matches!(reader.field()?, Field::Text(_)) {
            return Err(DispatchError::Unsupported);
        }
    }
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        reader.align4()?;
        if reader.dword()? != 0 {
            return Err(DispatchError::Unsupported);
        }
        let exstyle = reader.dword()?;
        let style = reader.dword()?;
        let units = [
            reader.word()?.cast_signed(),
            reader.word()?.cast_signed(),
            reader.word()?.cast_signed(),
            reader.word()?.cast_signed(),
        ];
        let id = reader.dword()?;
        let Field::Ordinal(class) = reader.field()? else {
            return Err(DispatchError::Unsupported);
        };
        if !matches!(class, 0x80 | 0x82 | 0x85) || style & 0x4000_0000 == 0 {
            return Err(DispatchError::Unsupported);
        }
        let title = reader.field()?;
        if let Field::Ordinal(resource) = &title
            && (class != 0x82 || *resource == 0)
        {
            return Err(DispatchError::Unsupported);
        }
        if reader.word()? != 0 {
            return Err(DispatchError::Unsupported);
        }
        items.push(Item {
            class,
            id,
            style,
            exstyle,
            title,
            units,
        });
    }
    Ok(Template {
        style,
        exstyle,
        title,
        units,
        items,
    })
}

impl Process32 {
    pub(super) fn create_dialog(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        let [instance, pointer, parent, procedure, _init] = args.try_into().unwrap();
        if instance == 0 || !self.modules.contains(instance) || parent != 0 || procedure == 0 {
            return Err(DispatchError::Unsupported);
        }
        let available = self
            .resources
            .loaded_payload_size(instance, pointer)
            .map_or(MAX_TEMPLATE, |size| size as usize);
        let template = parse(&self.memory, pointer, available)?;
        if template.units[2] <= 0 || template.units[3] <= 0 {
            return Err(DispatchError::Unsupported);
        }
        let Some(handle) = self.desktop.available_many(template.items.len() + 1) else {
            return Err(DispatchError::Unsupported);
        };
        let focus = template
            .items
            .iter()
            .position(|item| item.style & 0x0001_0000 != 0)
            .and_then(|index| u32::try_from(index + 1).ok())
            .and_then(|index| handle.checked_add(index * 4))
            .unwrap_or(0);
        self.start_dialog_callback(args, handle, focus, &template)?;
        self.desktop.insert(
            handle,
            desktop::Window {
                class: 0x8002,
                instance,
                procedure,
                style: template.style,
                exstyle: template.exstyle,
                title: template.title,
                dialog_units: Some(template.units),
                ..desktop::Window::default()
            },
        );
        for item in template.items {
            let child = self
                .desktop
                .available()
                .expect("preflighted dialog capacity");
            let title = match item.title {
                Field::Empty | Field::Ordinal(_) => String::new(),
                Field::Text(value) => value,
            };
            self.desktop.insert_child(
                child,
                desktop::Window {
                    class: u32::from(item.class),
                    instance,
                    style: item.style,
                    exstyle: item.exstyle,
                    title,
                    parent: handle,
                    id: item.id,
                    dialog_units: Some(item.units),
                    ..desktop::Window::default()
                },
            );
        }
        Ok(true)
    }

    fn start_dialog_callback(
        &mut self,
        args: &[u32],
        handle: u32,
        focus: u32,
        template: &Template,
    ) -> Result<(), DispatchError> {
        let [instance, _, parent, procedure, init] = args.try_into().unwrap();
        let caller = self.cpu.register(Register32::Esp);
        let hook = self.hooks.newest_cbt();
        self.callbacks
            .check_entry(hook.map_or(procedure, |(_, callback)| callback))?;
        let stack = if hook.is_some() {
            let base = caller
                .checked_sub(HOOK_SCRATCH)
                .ok_or(MemoryError::AddressOverflow)?;
            let lowest = base.checked_sub(20).ok_or(MemoryError::AddressOverflow)?;
            guest::check(
                &self.memory,
                lowest,
                (HOOK_SCRATCH + 20) as usize,
                Access::Write,
            )?;
            base
        } else {
            caller
        };
        let pending = Pending {
            window: handle,
            procedure,
            focus,
            init,
            phase: if hook.is_some() {
                Phase::Cbt
            } else {
                Phase::Init
            },
        };
        if hook.is_some() {
            let [x, y, width, height] =
                template.units.map(|value| i32::from(value).cast_unsigned());
            let creation = [
                init,
                instance,
                0,
                parent,
                height,
                width,
                y,
                x,
                template.style,
                0,
                0x8002,
                template.exstyle,
                stack,
                0,
            ];
            let bytes: Vec<_> = creation
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect();
            self.memory.write(u64::from(stack), &bytes)?;
        }
        let hook_arguments = [3, handle, stack + 48];
        let init_arguments = [handle, 0x110, focus, init];
        let arguments: &[u32] = if hook.is_some() {
            &hook_arguments
        } else {
            &init_arguments
        };
        self.callbacks.enter(
            &mut self.cpu,
            &mut self.memory,
            callbacks::Frame {
                stack,
                caller,
                cleanup: 24,
                creation: None,
                cbt_hook: hook.map(|(handle, _)| handle),
                module: None,
                dialog: Some(pending),
                destroy: None,
                paint: false,
            },
            hook.map_or(procedure, |(_, procedure)| procedure),
            arguments,
        )?;
        Ok(())
    }

    pub(super) fn finish_dialog_callback(
        &mut self,
        mut frame: callbacks::Frame,
        mut pending: Pending,
    ) -> Result<(), DispatchError> {
        if matches!(pending.phase, Phase::Cbt) {
            if self.cpu.register(Register32::Eax) != 0 {
                self.callbacks.finish(&mut self.cpu, &self.memory)?;
                self.desktop.remove(pending.window);
                self.cpu.set_register(Register32::Eax, 0);
                return Ok(());
            }
            pending.phase = Phase::Init;
            frame.dialog = Some(pending);
            frame.cbt_hook = None;
            return self.callbacks.replace(
                &mut self.cpu,
                &mut self.memory,
                frame,
                pending.procedure,
                &[pending.window, 0x110, pending.focus, pending.init],
            );
        }
        self.callbacks.finish(&mut self.cpu, &self.memory)?;
        self.desktop.activate_created(pending.window);
        self.cpu.set_register(Register32::Eax, pending.window);
        Ok(())
    }
}

struct Reader<'a> {
    memory: &'a GuestMemory,
    pointer: u32,
    available: usize,
    offset: usize,
}

impl Reader<'_> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], DispatchError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(DispatchError::Unsupported)?;
        if end > self.available {
            return Err(DispatchError::Unsupported);
        }
        let address = self
            .pointer
            .checked_add(u32::try_from(self.offset).map_err(|_| DispatchError::Unsupported)?)
            .ok_or(DispatchError::Unsupported)?;
        guest::check(self.memory, address, N, Access::Read)?;
        let mut value = [0; N];
        self.memory.read(u64::from(address), &mut value)?;
        self.offset = end;
        Ok(value)
    }

    fn word(&mut self) -> Result<u16, DispatchError> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }

    fn dword(&mut self) -> Result<u32, DispatchError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn align4(&mut self) -> Result<(), DispatchError> {
        self.offset = self
            .offset
            .checked_add(3)
            .ok_or(DispatchError::Unsupported)?
            & !3;
        if self.offset > self.available {
            return Err(DispatchError::Unsupported);
        }
        Ok(())
    }

    fn field(&mut self) -> Result<Field, DispatchError> {
        let first = self.word()?;
        match first {
            0 => Ok(Field::Empty),
            0xffff => Ok(Field::Ordinal(self.word()?)),
            _ => {
                let mut units = vec![first];
                loop {
                    if units.len() == MAX_TEXT {
                        return Err(DispatchError::Unsupported);
                    }
                    let unit = self.word()?;
                    if unit == 0 {
                        break;
                    }
                    units.push(unit);
                }
                String::from_utf16(&units)
                    .map(Field::Text)
                    .map_err(|_| DispatchError::Unsupported)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    fn fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [1_u16, 0xffff] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for dword in [0_u32, 0, 0x80c0_0000] {
            bytes.extend_from_slice(&dword.to_le_bytes());
        }
        for word in [1_u16, 0, 0, 100, 50, 0, 0, u16::from(b'T'), 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.resize((bytes.len() + 3) & !3, 0);
        for dword in [0_u32, 0, 0x5000_0000] {
            bytes.extend_from_slice(&dword.to_le_bytes());
        }
        for word in [4_u16, 5, 30, 12] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        for word in [0xffff_u16, 0x80, u16::from(b'O'), u16::from(b'K'), 0, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn extended_template_preserves_item_identity_and_bounds() {
        let bytes = fixture();
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x2000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x2000, &bytes).unwrap();
        let Ok(template) = parse(&memory, 0x2000, bytes.len()) else {
            panic!("authored extended template must parse");
        };
        assert_eq!(template.style, 0x80c0_0000);
        assert_eq!(template.title, "T");
        assert_eq!(template.units, [0, 0, 100, 50]);
        assert_eq!(template.items.len(), 1);
        assert_eq!(template.items[0].class, 0x80);
        assert_eq!(template.items[0].id, 42);
        assert_eq!(template.items[0].units, [4, 5, 30, 12]);
        assert!(matches!(
            &template.items[0].title,
            Field::Text(title) if title == "OK"
        ));
        assert!(matches!(
            parse(&memory, 0x2000, bytes.len() - 1),
            Err(DispatchError::Unsupported)
        ));
    }
}
