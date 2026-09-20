use super::{ARGC, Crt, DispatchError, GuestMemory, guest};
use crate::execution::Access;

pub(super) fn get_main(
    crt: &mut Crt,
    arguments: &[u32],
    memory: &mut GuestMemory,
) -> Result<(), DispatchError> {
    if arguments[3] != 0 {
        return Err(DispatchError::Unsupported);
    }
    let mut new_mode = [crt.new_mode];
    if arguments[4] != 0 {
        guest::read_words(memory, arguments[4], &mut new_mode)?;
        if new_mode[0] > 1 {
            return Err(DispatchError::Unsupported);
        }
    }
    let mut values = [0; 3];
    guest::read_words(memory, ARGC, &mut values)?;
    for &address in &arguments[..3] {
        guest::check(memory, address, 4, Access::Write)?;
    }
    for (&address, value) in arguments[..3].iter().zip(values) {
        guest::write_word(memory, address, value)?;
    }
    crt.new_mode = new_mode[0];
    Ok(())
}
