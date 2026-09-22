use super::super::Access;
use super::{
    DispatchError, GuestMemory, MemoryError, Process32, Register32, callbacks, desktop, gdi, guest,
    system, thread,
};

const SCRATCH: u32 = 112;
const CAPTION: u32 = 0x00c0_0000;
const CLIP_SIBLINGS: u32 = 0x0400_0000;
const WINDOW_EDGE: u32 = 0x100;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Create,
    Default,
    Rectangle,
    ClientRectangle,
    Parent,
    GetLong,
    SetLong,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x2a8 => Some(Self::Create),
            0x2ac => Some(Self::Default),
            0x2b0 => Some(Self::Rectangle),
            0x2b4 => Some(Self::ClientRectangle),
            0x2b8 => Some(Self::Parent),
            0x2bc => Some(Self::GetLong),
            0x2c0 => Some(Self::SetLong),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Create => 12,
            Self::Default => 4,
            Self::Parent => 1,
            Self::SetLong => 3,
            _ => 2,
        }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Cbt,
    MinMax,
    NcCreate,
    NcCalc,
    Create,
    Destroy,
    NcDestroy,
}

impl Phase {
    fn message(self) -> u32 {
        match self {
            Self::MinMax => 0x24,
            Self::NcCreate => 0x81,
            Self::NcCalc => 0x83,
            Self::Create => 1,
            Self::Destroy => 2,
            Self::NcDestroy => 0x82,
            Self::Cbt => unreachable!(),
        }
    }
    fn parameter(self, base: u32) -> u32 {
        match self {
            Self::MinMax => base + 56,
            Self::NcCreate | Self::Create => base,
            Self::NcCalc => base + 96,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Pending {
    window: u32,
    initial: [u32; 12],
    geometry: [i32; 4],
    phase: Phase,
}

impl Process32 {
    pub(super) fn create_window(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        if args[0] != 0
            || args[3] & !0x00cf_0000 != 0
            || args[8] != 0
            || args[9] != 0
            || args[4..6].contains(&0x8000_0000)
            || args[6].cast_signed() < 0
            || args[7].cast_signed() < 0
        {
            return Err(DispatchError::Unsupported);
        }
        let Some((atom, class)) = self.classes.find(
            args[10],
            args[1],
            &self.modules,
            &self.user_atoms,
            &self.memory,
        )?
        else {
            return self.creation_failure(1407);
        };
        desktop::read_title(&self.memory, args[2])?;
        let Some(handle) = self.desktop.available() else {
            return self.creation_failure(8);
        };
        let caller = self.cpu.register(Register32::Esp);
        let lowest = caller
            .checked_sub(SCRATCH + 20)
            .ok_or(MemoryError::AddressOverflow)?;
        guest::check(&self.memory, lowest, (SCRATCH + 20) as usize, Access::Write)?;
        let base = lowest + 20;
        let hook = self.hooks.newest_cbt();
        let procedure = hook.map_or(class[1], |(_, procedure)| procedure);
        self.callbacks.check_entry(procedure)?;
        let initial = [
            args[11], args[10], args[9], args[8], args[7], args[6], args[5], args[4], args[3],
            args[2], args[1], args[0],
        ];
        let phase = if hook.is_some() {
            Phase::Cbt
        } else {
            Phase::MinMax
        };
        let pending = Pending {
            window: handle,
            initial,
            geometry: [args[4], args[5], args[6], args[7]].map(u32::cast_signed),
            phase,
        };
        let frame = callbacks::Frame {
            stack: base,
            caller,
            cleanup: 52,
            creation: Some(pending),
        };
        let style = if hook.is_some() {
            args[3]
        } else {
            args[3] | CAPTION | CLIP_SIBLINGS
        };
        let window = desktop::Window {
            class: atom,
            instance: args[10],
            procedure: class[1],
            style,
            exstyle: if hook.is_some() { 0 } else { WINDOW_EDGE },
            ..desktop::Window::default()
        };
        let mut scratch = [0; 28];
        scratch[..12].copy_from_slice(&initial);
        scratch[12] = base;
        scratch[14..24].copy_from_slice(&minmax(args[3]));
        write_words(&mut self.memory, base, &scratch)?;
        let arguments = if hook.is_some() {
            [3, handle, base + 48, 0]
        } else {
            [handle, 0x24, 0, base + 56]
        };
        self.callbacks.enter(
            &mut self.cpu,
            &mut self.memory,
            frame,
            procedure,
            &arguments[..if hook.is_some() { 3 } else { 4 }],
        )?;
        self.desktop.insert(handle, window);
        Ok(true)
    }

    fn creation_failure(&mut self, error: u32) -> Result<bool, DispatchError> {
        thread::set_last_error(&mut self.memory, error)?;
        self.cpu.set_register(Register32::Eax, 0);
        Ok(false)
    }

    pub(super) fn finish_callback(&mut self) -> Result<(), DispatchError> {
        let frame = self.callbacks.current(&self.cpu)?;
        let Some(pending) = frame.creation else {
            return self.callbacks.finish(&mut self.cpu, &self.memory);
        };
        let result = self.cpu.register(Register32::Eax);
        match pending.phase {
            Phase::Cbt if result != 0 => self.finish_creation(pending, false),
            Phase::Cbt => self.after_cbt(frame, pending),
            Phase::MinMax => self.after_minmax(frame, pending),
            Phase::NcCreate if result == 0 => self.deliver(frame, pending, Phase::NcDestroy),
            Phase::NcCreate => self.calculate_client(frame, pending),
            Phase::NcCalc => self.after_client(frame, pending),
            Phase::Create if result == u32::MAX => self.deliver(frame, pending, Phase::Destroy),
            Phase::Create => self.finish_creation(pending, true),
            Phase::Destroy => self.deliver(frame, pending, Phase::NcDestroy),
            Phase::NcDestroy => self.finish_creation(pending, false),
        }
    }

    fn finish_creation(&mut self, pending: Pending, success: bool) -> Result<(), DispatchError> {
        self.callbacks.finish(&mut self.cpu, &self.memory)?;
        if !success {
            self.desktop.remove(pending.window);
        }
        self.cpu
            .set_register(Register32::Eax, if success { pending.window } else { 0 });
        Ok(())
    }

    fn delivery_target(
        &self,
        frame: callbacks::Frame,
        pending: Pending,
    ) -> Result<u32, DispatchError> {
        let window = self
            .desktop
            .window(pending.window)
            .ok_or(DispatchError::Unsupported)?;
        if window.procedure == callbacks::RETURN {
            return Err(DispatchError::Unsupported);
        }
        guest::check(&self.memory, frame.stack - 20, 20, Access::Write)?;
        Ok(window.procedure)
    }

    fn deliver(
        &mut self,
        mut frame: callbacks::Frame,
        mut pending: Pending,
        phase: Phase,
    ) -> Result<(), DispatchError> {
        let procedure = self.delivery_target(frame, pending)?;
        pending.phase = phase;
        frame.creation = Some(pending);
        self.callbacks.replace(
            &mut self.cpu,
            &mut self.memory,
            frame,
            procedure,
            &[
                pending.window,
                phase.message(),
                0,
                phase.parameter(frame.stack),
            ],
        )
    }

    fn after_cbt(
        &mut self,
        frame: callbacks::Frame,
        mut pending: Pending,
    ) -> Result<(), DispatchError> {
        let mut current = [0; 14];
        guest::read_words(&self.memory, frame.stack, &mut current)?;
        if current[12] != frame.stack
            || current[13] != 0
            || current[..12]
                .iter()
                .enumerate()
                .any(|(index, &value)| !(4..8).contains(&index) && value != pending.initial[index])
            || current[4].cast_signed() < 0
            || current[5].cast_signed() < 0
            || current[6..8].contains(&0x8000_0000)
        {
            return Err(DispatchError::Unsupported);
        }
        pending.geometry = [current[7], current[6], current[5], current[4]].map(u32::cast_signed);
        self.delivery_target(frame, pending)?;
        write_words(
            &mut self.memory,
            frame.stack + 56,
            &minmax(pending.initial[8]),
        )?;
        self.deliver(frame, pending, Phase::MinMax)?;
        let window = self
            .desktop
            .window_mut(pending.window)
            .expect("active creation owns window");
        window.style |= CAPTION | CLIP_SIBLINGS;
        window.exstyle = WINDOW_EDGE;
        Ok(())
    }

    fn after_minmax(
        &mut self,
        frame: callbacks::Frame,
        pending: Pending,
    ) -> Result<(), DispatchError> {
        let mut limits = [0; 10];
        guest::read_words(&self.memory, frame.stack + 56, &mut limits)?;
        let mut rectangle = [pending.geometry[0], pending.geometry[1], 0, 0];
        for axis in 0..2 {
            let min = limits[6 + axis].cast_signed();
            let max = limits[8 + axis].cast_signed().max(min);
            let size = pending.geometry[axis + 2].clamp(min, max).max(0);
            rectangle[axis + 2] = rectangle[axis].saturating_add(size);
        }
        self.deliver(frame, pending, Phase::NcCreate)?;
        let window = self
            .desktop
            .window_mut(pending.window)
            .expect("active creation owns window");
        window.rectangle = rectangle;
        window.client = rectangle;
        Ok(())
    }

    fn calculate_client(
        &mut self,
        frame: callbacks::Frame,
        pending: Pending,
    ) -> Result<(), DispatchError> {
        self.delivery_target(frame, pending)?;
        let rectangle = self
            .desktop
            .window(pending.window)
            .expect("validated window")
            .rectangle;
        write_words(
            &mut self.memory,
            frame.stack + 96,
            &rectangle.map(i32::cast_unsigned),
        )?;
        self.deliver(frame, pending, Phase::NcCalc)
    }

    fn after_client(
        &mut self,
        frame: callbacks::Frame,
        pending: Pending,
    ) -> Result<(), DispatchError> {
        let rectangle = read_rectangle(&self.memory, frame.stack + 96)?;
        self.deliver(frame, pending, Phase::Create)?;
        self.desktop
            .window_mut(pending.window)
            .expect("active creation owns window")
            .client = rectangle;
        Ok(())
    }

    pub(super) fn window_api(&mut self, call: Call, args: &[u32]) -> Result<(), DispatchError> {
        let result = match call {
            Call::Default => self.default_window_proc(args)?,
            Call::Rectangle => self.window_rectangle(args, false)?,
            Call::ClientRectangle => self.window_rectangle(args, true)?,
            Call::Parent | Call::GetLong | Call::SetLong => self.window_property(call, args)?,
            Call::Create => unreachable!(),
        };
        self.cpu.set_register(Register32::Eax, result);
        Ok(())
    }

    fn window_property(&mut self, call: Call, args: &[u32]) -> Result<u32, DispatchError> {
        if matches!(call, Call::Parent) {
            if args[0] == desktop::DESKTOP {
                return Ok(0);
            }
        } else if args[1] != (-4_i32).cast_unsigned() || args[0] == desktop::DESKTOP {
            return Err(DispatchError::Unsupported);
        }
        if matches!(call, Call::SetLong) && matches!(args[2], 0 | callbacks::RETURN) {
            return Err(DispatchError::Unsupported);
        }
        let Some(window) = self.desktop.window_mut(args[0]) else {
            thread::set_last_error(&mut self.memory, 1400)?;
            return Ok(0);
        };
        match call {
            // creation currently admits only unowned, non-popup top-level windows.
            Call::Parent => Ok(0),
            Call::GetLong => Ok(window.procedure),
            Call::SetLong => Ok(std::mem::replace(&mut window.procedure, args[2])),
            _ => unreachable!(),
        }
    }

    fn default_window_proc(&mut self, args: &[u32]) -> Result<u32, DispatchError> {
        let Some(window) = self.desktop.window(args[0]) else {
            thread::set_last_error(&mut self.memory, 1400)?;
            return Ok(0);
        };
        match args[1] {
            0x81 => {
                let mut creation = [0; 12];
                guest::read_words(&self.memory, args[3], &mut creation)?;
                let title = desktop::read_title(&self.memory, creation[9])?;
                self.desktop
                    .window_mut(args[0])
                    .expect("validated window")
                    .title = title;
                Ok(1)
            }
            0x83 if args[2] == 0 => {
                let frame = if window.style & 0x0004_0000 != 0 {
                    system::RESIZE_FRAME
                } else if window.exstyle & WINDOW_EDGE != 0 || window.style & 0x0040_0000 != 0 {
                    system::DIALOG_FRAME
                } else {
                    i32::from(window.style & 0x0080_0000 != 0)
                };
                let caption = if window.style & CAPTION == CAPTION {
                    system::CAPTION_HEIGHT.cast_signed()
                } else {
                    0
                };
                let mut rectangle = read_rectangle(&self.memory, args[3])?;
                rectangle[0] = rectangle[0].saturating_add(frame);
                rectangle[1] = rectangle[1].saturating_add(frame + caption);
                rectangle[2] = rectangle[2].saturating_sub(frame).max(rectangle[0]);
                rectangle[3] = rectangle[3].saturating_sub(frame).max(rectangle[1]);
                write_words(
                    &mut self.memory,
                    args[3],
                    &rectangle.map(i32::cast_unsigned),
                )?;
                Ok(0)
            }
            0x24 | 1 | 2 | 0x82 => Ok(0),
            _ => Err(DispatchError::Unsupported),
        }
    }

    fn window_rectangle(&mut self, args: &[u32], client: bool) -> Result<u32, DispatchError> {
        let rectangle = if args[0] == desktop::DESKTOP {
            [
                0,
                0,
                gdi::SCREEN_WIDTH.cast_signed(),
                gdi::SCREEN_HEIGHT.cast_signed(),
            ]
        } else {
            let Some(window) = self.desktop.window(args[0]) else {
                thread::set_last_error(&mut self.memory, 1400)?;
                return Ok(0);
            };
            if client {
                [
                    0,
                    0,
                    window.client[2] - window.client[0],
                    window.client[3] - window.client[1],
                ]
            } else {
                window.rectangle
            }
        };
        write_words(
            &mut self.memory,
            args[1],
            &rectangle.map(i32::cast_unsigned),
        )?;
        Ok(1)
    }
}

fn minmax(style: u32) -> [u32; 10] {
    let frame = if style & 0x0004_0000 != 0 {
        system::RESIZE_FRAME
    } else {
        system::DIALOG_FRAME
    };
    [
        0,
        0,
        gdi::SCREEN_WIDTH.cast_signed() + 2 * frame,
        gdi::SCREEN_HEIGHT.cast_signed() + 2 * frame,
        -frame,
        -frame,
        system::MIN_TRACK[0],
        system::MIN_TRACK[1],
        system::MAX_TRACK[0],
        system::MAX_TRACK[1],
    ]
    .map(i32::cast_unsigned)
}

fn read_rectangle(memory: &GuestMemory, pointer: u32) -> Result<[i32; 4], DispatchError> {
    let mut words = [0; 4];
    guest::read_words(memory, pointer, &mut words)?;
    let rectangle = words.map(u32::cast_signed);
    for axis in 0..2 {
        if rectangle[axis + 2]
            .checked_sub(rectangle[axis])
            .is_none_or(|length| length < 0)
        {
            return Err(DispatchError::Unsupported);
        }
    }
    Ok(rectangle)
}

fn write_words(memory: &mut GuestMemory, pointer: u32, words: &[u32]) -> Result<(), DispatchError> {
    let data: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    guest::check(memory, pointer, data.len(), Access::Write)?;
    memory.write(u64::from(pointer), &data)?;
    Ok(())
}
