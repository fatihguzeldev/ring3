use super::LoaderEntry32;
use crate::{
    loader::ModuleId32,
    memory::{GuestAddress, WordWrite32},
    windows::process::ProcessError32,
};

pub(super) fn list_words(
    peb: u32,
    entries: &[LoaderEntry32],
    initialized: &[ModuleId32],
) -> Result<Vec<WordWrite32>, ProcessError32> {
    let mut words = Vec::new();
    words
        .try_reserve_exact(6 + entries.len() * 6)
        .map_err(|_| ProcessError32::Allocation)?;
    for (list, head_offset) in [0x20c, 0x214, 0x21c].into_iter().enumerate() {
        let head = peb + head_offset;
        let mut previous = head;
        let mut push = |entry: &LoaderEntry32| {
            let address = entry.address + list as u32 * 8;
            words.push(WordWrite32 {
                address: GuestAddress(previous),
                value: address,
            });
            words.push(WordWrite32 {
                address: GuestAddress(address + 4),
                value: previous,
            });
            previous = address;
        };
        if list == 2 {
            for id in initialized {
                let entry = entries
                    .iter()
                    .find(|entry| entry.id == *id && entry.dll)
                    .ok_or(ProcessError32::InvalidParameter)?;
                push(entry);
            }
        } else {
            for entry in entries {
                push(entry);
            }
        }
        words.push(WordWrite32 {
            address: GuestAddress(previous),
            value: head,
        });
        words.push(WordWrite32 {
            address: GuestAddress(head + 4),
            value: previous,
        });
    }
    Ok(words)
}
