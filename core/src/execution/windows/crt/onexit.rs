use super::{Access, DispatchError, GuestMemory, guest, heap};

#[derive(Default)]
pub(super) struct Registry {
    functions: Vec<u32>,
}

impl Registry {
    pub(super) fn register(&mut self, function: u32) -> u32 {
        if function == 0 || self.functions.len() == 4096 || self.functions.try_reserve(1).is_err() {
            return 0;
        }
        self.functions.push(function);
        function
    }
}

pub(super) fn register(
    heap: &mut heap::Heap,
    memory: &mut GuestMemory,
    stack: u32,
    arguments: &[u32],
    errno: u32,
) -> Result<u32, DispatchError> {
    let (function, begin_cell, end_cell) = (arguments[0], arguments[1], arguments[2]);
    if begin_cell == 0 || end_cell == 0 {
        return Ok(0);
    }
    let (mut begin, mut end) = ([0], [0]);
    guest::read_words(memory, begin_cell, &mut begin)?;
    guest::read_words(memory, end_cell, &mut end)?;
    let (begin, end) = (begin[0], end[0]);
    if begin == 0 || end == 0 {
        return Ok(0);
    }
    let length = heap.crt_length(begin).ok_or(DispatchError::Unsupported)?;
    let used = end
        .checked_sub(begin)
        .filter(|used| used % 4 == 0 && u64::from(*used) <= length)
        .ok_or(DispatchError::Unsupported)?;
    if overlaps(begin_cell, 4, end_cell, 4)
        || overlaps(begin, length, begin_cell, 4)
        || overlaps(begin, length, end_cell, 4)
        || overlaps(begin, length, stack, 16)
    {
        return Err(DispatchError::Unsupported);
    }
    guest::check(memory, begin_cell, 4, Access::Write)?;
    guest::check(memory, end_cell, 4, Access::Write)?;
    let needed = used + 4;
    let destination = if u64::from(needed) <= length {
        guest::check(memory, end, 4, Access::Write)?;
        begin
    } else {
        guest::check(
            memory,
            begin,
            usize::try_from(used).expect("heap span fits usize"),
            Access::Read,
        )?;
        let Some(new) = heap.allocate_crt(needed, memory)? else {
            guest::write_word(memory, errno, 12)?;
            return Ok(0);
        };
        // all remaining ranges are checked or belong to the fresh allocation.
        let mut buffer = [0; 1024];
        let mut offset = 0;
        while offset < used {
            let count = (used - offset).min(1024);
            let bytes = &mut buffer[..usize::try_from(count).expect("chunk fits usize")];
            memory.read(u64::from(begin) + u64::from(offset), bytes)?;
            memory.write(u64::from(new) + u64::from(offset), bytes)?;
            offset += count;
        }
        heap.free_crt(begin, stack, memory)?;
        new
    };
    guest::write_word(memory, destination + used, function)?;
    guest::write_word(memory, begin_cell, destination)?;
    guest::write_word(memory, end_cell, destination + needed)?;
    Ok(function)
}

fn overlaps(left: u32, left_length: u64, right: u32, right_length: u64) -> bool {
    u64::from(left) < u64::from(right) + right_length
        && u64::from(right) < u64::from(left) + left_length
}

#[cfg(test)]
mod tests {
    use super::super::Crt;

    #[test]
    fn crt_registry_retains_order_duplicates_and_independent_ownership() {
        let mut first = Crt::default();
        let second = Crt::default();
        for function in [42, 17, 42] {
            assert_eq!(first.exit_callbacks.register(function), function);
        }
        assert_eq!(first.exit_callbacks.register(0), 0);
        assert_eq!(first.exit_callbacks.functions, [42, 17, 42]);
        assert!(second.exit_callbacks.functions.is_empty());
        for function in 1..=4093 {
            assert_eq!(first.exit_callbacks.register(function), function);
        }
        let before = first.exit_callbacks.functions.clone();
        assert_eq!(first.exit_callbacks.register(99), 0);
        assert_eq!(first.exit_callbacks.functions, before);
    }
}
