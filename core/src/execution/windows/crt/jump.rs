use super::{Access, Cpu32, DispatchError, GuestMemory, MemoryError, Register32, guest};

pub(super) fn capture(
    cpu: &Cpu32,
    memory: &mut GuestMemory,
    args: &[u32],
) -> Result<u32, DispatchError> {
    let (destination, count) = (args[0], args[1]);
    if destination == 0 || count > 8 {
        return Err(DispatchError::Unsupported);
    }
    let count = usize::try_from(count).expect("bounded optional arguments");
    let stack = cpu.register(Register32::Esp);
    let mut frame = [0; 11];
    guest::read_words(memory, stack, &mut frame[..count + 3])?;
    let mut registration = [0];
    guest::read_words(memory, cpu.fs_base(), &mut registration)?;
    let mut context = [0; 16];
    for (index, register) in [
        Register32::Ebp,
        Register32::Ebx,
        Register32::Edi,
        Register32::Esi,
    ]
    .into_iter()
    .enumerate()
    {
        context[index] = cpu.register(register);
    }
    context[4..10].copy_from_slice(&[stack, frame[0], registration[0], u32::MAX, 0x5643_3230, 0]);
    let mut data_count = 0;
    if registration[0] != u32::MAX {
        if count != 0 {
            context[9] = frame[3];
        }
        if count >= 2 {
            context[7] = frame[4];
        } else {
            let level = registration[0]
                .checked_add(12)
                .ok_or(MemoryError::AddressOverflow)?;
            guest::read_words(memory, level, &mut context[7..8])?;
        }
        data_count = count.saturating_sub(2);
        context[10..10 + data_count].copy_from_slice(&frame[5..5 + data_count]);
    }
    let length = (10 + data_count) * 4;
    if u64::from(destination) < u64::from(stack) + ((count + 3) * 4) as u64
        && u64::from(stack) < u64::from(destination) + length as u64
    {
        return Err(DispatchError::Unsupported);
    }
    guest::check(memory, destination, length, Access::Write)?;
    let mut bytes = [0; 64];
    for (word, output) in context.iter().zip(bytes.chunks_exact_mut(4)) {
        output.copy_from_slice(&word.to_le_bytes());
    }
    memory.write(u64::from(destination), &bytes[..length])?;
    Ok(0)
}
