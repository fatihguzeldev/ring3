use super::{API_BASE, Access, Cpu32, DispatchError, GuestMemory, Process32, Register32, guest};

pub(super) const RETURN: u32 = API_BASE + 0xff4;
const FRAME_HANDLER: u32 = API_BASE + 0x494;
const MAX_STATES: u32 = 64;
const MAX_TRY_BLOCKS: u32 = 32;

pub(super) struct Pending {
    inner: Option<Inner>,
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

#[derive(Clone, Copy)]
struct Inner {
    record: u32,
    frame: u32,
    action: u32,
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
    throw_info: [u32; 4],
    allow_typed: bool,
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
    if catch[0] != 0 || catch[2] != 0 || catch[3] == 0 {
        return Err(DispatchError::Unsupported);
    }
    if catch[1] != 0 {
        if !allow_typed || throw_info[0] != 0 || throw_info[2] != 0 || throw_info[3] == 0 {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, catch[1], 8, Access::Read)?;
        let type_count = words::<1>(memory, throw_info[3])?[0];
        if type_count == 0 || type_count > 32 {
            return Err(DispatchError::Unsupported);
        }
        let type_entries = throw_info[3]
            .checked_add(4)
            .ok_or(DispatchError::Unsupported)?;
        let mut matched = false;
        for index in 0..type_count {
            let candidate_address = words::<1>(memory, entry(type_entries, index, 4)?)?[0];
            let candidate = words::<7>(memory, candidate_address)?;
            if candidate[1] == catch[1]
                && candidate[0] == 1
                && candidate[2..5] == [0, u32::MAX, 0]
                && candidate[5] == 4
                && candidate[6] == 0
            {
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(DispatchError::Unsupported);
        }
    }
    guest::check(memory, catch[3], 1, Access::Execute)?;
    Ok((try_low, catch[3]))
}

fn inspect_inner(
    cpu: &Cpu32,
    memory: &GuestMemory,
    record: u32,
) -> Result<(u32, Inner), DispatchError> {
    let registration = words::<3>(memory, record)?;
    if registration[0] == u32::MAX || registration[0] <= record {
        return Err(DispatchError::Unsupported);
    }
    let frame = record.checked_add(12).ok_or(DispatchError::Unsupported)?;
    let outer_frame = registration[0]
        .checked_add(12)
        .ok_or(DispatchError::Unsupported)?;
    if cpu.register(Register32::Ebp) != outer_frame
        || cpu.register(Register32::Esp) >= record
        || frame >= registration[0]
    {
        return Err(DispatchError::Unsupported);
    }
    let info_address = func_info_address(memory, registration[1])?;
    let info = words::<7>(memory, info_address)?;
    if !matches!(info[0], 0x1993_0520 | 0x1993_0522)
        || info[1] == 0
        || info[1] > MAX_STATES
        || info[3] != 0
        || info[4] != 0
        || registration[2] >= info[1]
    {
        return Err(DispatchError::Unsupported);
    }
    let unwind = words::<2>(memory, entry(info[2], registration[2], 8)?)?;
    if unwind[0] != u32::MAX || unwind[1] == 0 {
        return Err(DispatchError::Unsupported);
    }
    guest::check(memory, unwind[1], 1, Access::Execute)?;
    guest::check(memory, record + 8, 4, Access::Write)?;
    guest::check(memory, cpu.fs_base(), 4, Access::Write)?;
    Ok((
        registration[0],
        Inner {
            record,
            frame,
            action: unwind[1],
        },
    ))
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

        let mut record = words::<1>(memory, cpu.fs_base())?[0];
        if record == u32::MAX {
            return Err(DispatchError::Unsupported);
        }
        let inner = if cpu.register(Register32::Ebp)
            == record.checked_add(12).ok_or(DispatchError::Unsupported)?
        {
            None
        } else {
            let (outer, inner) = inspect_inner(cpu, memory, record)?;
            record = outer;
            Some(inner)
        };
        let frame = record.checked_add(12).ok_or(DispatchError::Unsupported)?;
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
        let (try_low, catch) = catch_for_state(memory, info, state, throw_info, inner.is_some())?;

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
            inner,
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

    fn check_outer_writes(&self, memory: &GuestMemory) -> Result<(), DispatchError> {
        guest::check(memory, self.record + 8, 4, Access::Write)?;
        guest::check(memory, self.call_stack - 4, 4, Access::Write)?;
        Ok(())
    }

    fn advance(&mut self, cpu: &mut Cpu32, memory: &mut GuestMemory) -> Result<(), DispatchError> {
        if let Some(inner) = self.inner {
            guest::write_word(memory, self.call_stack - 4, RETURN)?;
            cpu.set_register(Register32::Ebp, inner.frame);
            cpu.set_register(Register32::Esp, self.call_stack - 4);
            cpu.eip = inner.action;
            return Ok(());
        }
        let action = self.actions.get(self.next_action).copied();
        let (state, target) = action.unwrap_or((self.try_low, self.catch));
        self.check_outer_writes(memory)?;
        guest::write_word(memory, self.record + 8, state)?;
        guest::write_word(memory, self.call_stack - 4, RETURN)?;
        if action.is_some() {
            self.next_action += 1;
        } else {
            self.catch_started = true;
        }
        cpu.set_register(Register32::Ebp, self.frame);
        cpu.set_register(Register32::Esp, self.call_stack - 4);
        cpu.eip = target;
        Ok(())
    }
}

impl Process32 {
    pub(super) fn throw_exception(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        let exception = &mut self.threads.state_mut(self.cpu.fs_base())?.exception;
        if exception.is_some() {
            return Err(DispatchError::Unsupported);
        }
        let mut pending = Pending::inspect(&self.cpu, &self.memory, args)?;
        pending.advance(&mut self.cpu, &mut self.memory)?;
        *exception = Some(pending);
        Ok(())
    }

    pub(super) fn finish_exception_call(&mut self) -> Result<(), DispatchError> {
        let exception = &mut self.threads.state_mut(self.cpu.fs_base())?.exception;
        let Some(mut pending) = exception.take() else {
            return Err(DispatchError::Unsupported);
        };
        let expected_frame = pending.inner.map_or(pending.frame, |inner| inner.frame);
        if self.cpu.register(Register32::Esp) != pending.call_stack
            || self.cpu.register(Register32::Ebp) != expected_frame
        {
            *exception = Some(pending);
            return Err(DispatchError::Unsupported);
        }
        if let Some(inner) = pending.inner {
            let result = (|| {
                if words::<1>(&self.memory, self.cpu.fs_base())?[0] != inner.record {
                    return Err(DispatchError::Unsupported);
                }
                guest::check(&self.memory, inner.record + 8, 4, Access::Write)?;
                guest::check(&self.memory, self.cpu.fs_base(), 4, Access::Write)?;
                pending.check_outer_writes(&self.memory)?;
                guest::write_word(&mut self.memory, inner.record + 8, u32::MAX)?;
                guest::write_word(&mut self.memory, self.cpu.fs_base(), pending.record)?;
                pending.inner = None;
                pending.advance(&mut self.cpu, &mut self.memory)
            })();
            *exception = Some(pending);
            result?;
            return Ok(());
        }
        if pending.catch_started {
            let continuation = self.cpu.register(Register32::Eax);
            if let Err(error) = guest::check(&self.memory, continuation, 1, Access::Execute) {
                *exception = Some(pending);
                return Err(error.into());
            }
            self.cpu.set_register(Register32::Esp, pending.saved_stack);
            self.cpu.eip = continuation;
        } else {
            let result = pending.advance(&mut self.cpu, &mut self.memory);
            *exception = Some(pending);
            result?;
        }
        Ok(())
    }
}
