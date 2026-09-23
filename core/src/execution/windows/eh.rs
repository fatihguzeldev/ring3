use super::{API_BASE, Access, Cpu32, DispatchError, GuestMemory, Process32, Register32, guest};

pub(super) const RETURN: u32 = API_BASE + 0xff4;
const FRAME_HANDLER: u32 = API_BASE + 0x494;
const MAX_STATES: u32 = 64;
const MAX_TRY_BLOCKS: u32 = 32;

pub(super) struct Pending {
    record: u32,
    frame: u32,
    call_stack: u32,
    saved_stack: u32,
    try_low: u32,
    catch: u32,
    actions: Vec<(u32, u32)>,
    next_action: usize,
    catch_started: bool,
}

fn words<const N: usize>(memory: &GuestMemory, address: u32) -> Result<[u32; N], DispatchError> {
    let mut values = [0; N];
    guest::read_words(memory, address, &mut values)?;
    Ok(values)
}

fn entry(base: u32, index: u32, stride: u32) -> Result<u32, DispatchError> {
    base.checked_add(
        index
            .checked_mul(stride)
            .ok_or(DispatchError::Unsupported)?,
    )
    .ok_or(DispatchError::Unsupported)
}

fn func_info_address(memory: &GuestMemory, handler: u32) -> Result<u32, DispatchError> {
    let mut thunk = [0; 11];
    memory.fetch(u64::from(handler), &mut thunk)?;
    if thunk[0] != 0xb8 {
        return Err(DispatchError::Unsupported);
    }
    let handler_slot = if thunk[5..7] == [0xff, 0x25] {
        u32::from_le_bytes(thunk[7..11].try_into().unwrap())
    } else if thunk[5] == 0xe9 {
        let displacement = i32::from_le_bytes(thunk[6..10].try_into().unwrap());
        let jump = handler.wrapping_add(10).wrapping_add_signed(displacement);
        let mut import_thunk = [0; 6];
        memory.fetch(u64::from(jump), &mut import_thunk)?;
        if import_thunk[..2] != [0xff, 0x25] {
            return Err(DispatchError::Unsupported);
        }
        u32::from_le_bytes(import_thunk[2..6].try_into().unwrap())
    } else {
        return Err(DispatchError::Unsupported);
    };
    if words::<1>(memory, handler_slot)?[0] != FRAME_HANDLER {
        return Err(DispatchError::Unsupported);
    }
    Ok(u32::from_le_bytes(thunk[1..5].try_into().unwrap()))
}

fn catch_for_state(
    memory: &GuestMemory,
    info: [u32; 7],
    state: u32,
) -> Result<(u32, u32), DispatchError> {
    let mut selected = None;
    for index in 0..info[3] {
        let block = words::<5>(memory, entry(info[4], index, 20)?)?;
        if block[0] > block[1]
            || block[1] >= block[2]
            || block[2] >= info[1]
            || block[3] == 0
            || block[3] > 32
        {
            return Err(DispatchError::Unsupported);
        }
        if !(block[0]..=block[1]).contains(&state) {
            continue;
        }
        if block[3] != 1 {
            return Err(DispatchError::Unsupported);
        }
        if selected.is_none_or(|(low, _): (u32, u32)| block[0] > low) {
            selected = Some((block[0], block[4]));
        }
    }
    let (try_low, handlers) = selected.ok_or(DispatchError::Unsupported)?;
    let catch = words::<4>(memory, handlers)?;
    if catch[0] != 0 || catch[1] != 0 || catch[2] != 0 || catch[3] == 0 {
        return Err(DispatchError::Unsupported);
    }
    guest::check(memory, catch[3], 1, Access::Execute)?;
    Ok((try_low, catch[3]))
}

impl Pending {
    fn inspect(cpu: &Cpu32, memory: &GuestMemory, args: &[u32]) -> Result<Self, DispatchError> {
        if args[0] == 0 || args[1] == 0 {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, args[0], 1, Access::Read)?;
        let throw_info = words::<4>(memory, args[1])?;
        if throw_info[1] != 0 {
            return Err(DispatchError::Unsupported);
        }

        let record = words::<1>(memory, cpu.fs_base())?[0];
        let frame = record.checked_add(12).ok_or(DispatchError::Unsupported)?;
        if record == u32::MAX || cpu.register(Register32::Ebp) != frame {
            return Err(DispatchError::Unsupported);
        }
        let registration = words::<3>(memory, record)?;
        if registration[0] != u32::MAX && registration[0] <= record {
            return Err(DispatchError::Unsupported);
        }
        let info_address = func_info_address(memory, registration[1])?;
        let info = words::<7>(memory, info_address)?;
        if !matches!(info[0], 0x1993_0520 | 0x1993_0522)
            || info[1] == 0
            || info[1] > MAX_STATES
            || info[3] == 0
            || info[3] > MAX_TRY_BLOCKS
            || registration[2] >= info[1]
        {
            return Err(DispatchError::Unsupported);
        }

        let state = registration[2];
        let (try_low, catch) = catch_for_state(memory, info, state)?;

        let mut actions = Vec::new();
        let mut cursor = state;
        while cursor > try_low {
            let unwind = words::<2>(memory, entry(info[2], cursor, 8)?)?;
            if unwind[0] < try_low || unwind[0] >= cursor {
                return Err(DispatchError::Unsupported);
            }
            if unwind[1] != 0 {
                guest::check(memory, unwind[1], 1, Access::Execute)?;
                actions.push((unwind[0], unwind[1]));
            }
            cursor = unwind[0];
        }

        let call_stack = cpu.register(Register32::Esp);
        let saved_stack = words::<1>(
            memory,
            record.checked_sub(4).ok_or(DispatchError::Unsupported)?,
        )?[0];
        if saved_stack <= call_stack || saved_stack >= record || record - saved_stack > 16 * 1024 {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, record + 8, 4, Access::Write)?;
        guest::check(
            memory,
            call_stack
                .checked_sub(4)
                .ok_or(DispatchError::Unsupported)?,
            4,
            Access::Write,
        )?;
        guest::check(memory, saved_stack, 4, Access::Read)?;
        Ok(Self {
            record,
            frame,
            call_stack,
            saved_stack,
            try_low,
            catch,
            actions,
            next_action: 0,
            catch_started: false,
        })
    }

    fn advance(&mut self, cpu: &mut Cpu32, memory: &mut GuestMemory) -> Result<(), DispatchError> {
        let (state, target) = if let Some(&(state, target)) = self.actions.get(self.next_action) {
            self.next_action += 1;
            (state, target)
        } else {
            self.catch_started = true;
            (self.try_low, self.catch)
        };
        guest::write_word(memory, self.record + 8, state)?;
        guest::write_word(memory, self.call_stack - 4, RETURN)?;
        cpu.set_register(Register32::Ebp, self.frame);
        cpu.set_register(Register32::Esp, self.call_stack - 4);
        cpu.eip = target;
        Ok(())
    }
}

impl Process32 {
    pub(super) fn throw_exception(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if self.exception.is_some() {
            return Err(DispatchError::Unsupported);
        }
        let mut pending = Pending::inspect(&self.cpu, &self.memory, args)?;
        pending.advance(&mut self.cpu, &mut self.memory)?;
        self.exception = Some(pending);
        Ok(())
    }

    pub(super) fn finish_exception_call(&mut self) -> Result<(), DispatchError> {
        let Some(mut pending) = self.exception.take() else {
            return Err(DispatchError::Unsupported);
        };
        if self.cpu.register(Register32::Esp) != pending.call_stack
            || self.cpu.register(Register32::Ebp) != pending.frame
        {
            self.exception = Some(pending);
            return Err(DispatchError::Unsupported);
        }
        if pending.catch_started {
            let continuation = self.cpu.register(Register32::Eax);
            if let Err(error) = guest::check(&self.memory, continuation, 1, Access::Execute) {
                self.exception = Some(pending);
                return Err(error.into());
            }
            self.cpu.set_register(Register32::Esp, pending.saved_stack);
            self.cpu.eip = continuation;
        } else {
            let result = pending.advance(&mut self.cpu, &mut self.memory);
            self.exception = Some(pending);
            result?;
        }
        Ok(())
    }
}
