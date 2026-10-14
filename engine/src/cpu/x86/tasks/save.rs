use super::{TaskFormat, TaskSave};

pub const MAX_TASK_SAVE_WORDS: usize = 26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSaveWord {
    pub offset: u16,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSavePlan {
    words: [TaskSaveWord; MAX_TASK_SAVE_WORDS],
    count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSaveOverlayAuthority {
    /// all aliases relevant to this image are identified by the same wrapping linear address.
    SameLinearAddressSpace,
    UnidentifiedPhysicalAliases,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSaveOverlayError {
    UnidentifiedPhysicalAliases,
    ImageTooLarge,
}

impl TaskSavePlan {
    pub fn new(format: TaskFormat, save: &TaskSave) -> Self {
        let mut plan = Self {
            words: [TaskSaveWord {
                offset: 0,
                value: 0,
            }; MAX_TASK_SAVE_WORDS],
            count: 0,
        };
        let flags = save.flags & !0x4000;
        match format {
            TaskFormat::Word => {
                for (index, value) in [save.next_eip, flags]
                    .into_iter()
                    .chain(save.general)
                    .enumerate()
                {
                    plan.push(14 + index as u16 * 2, value as u16);
                }
                for (index, value) in save.selectors[..4].iter().copied().enumerate() {
                    plan.push(34 + index as u16 * 2, value);
                }
            }
            TaskFormat::Dword => {
                for (index, value) in [save.next_eip, flags]
                    .into_iter()
                    .chain(save.general)
                    .enumerate()
                {
                    let offset = 32 + index as u16 * 4;
                    plan.push(offset, value as u16);
                    plan.push(offset + 2, (value >> 16) as u16);
                }
                for (index, value) in save.selectors.iter().copied().enumerate() {
                    plan.push(72 + index as u16 * 4, value);
                }
            }
        }
        plan
    }

    fn push(&mut self, offset: u16, value: u16) {
        self.words[self.count] = TaskSaveWord { offset, value };
        self.count += 1;
    }

    pub fn words(&self) -> &[TaskSaveWord] {
        &self.words[..self.count]
    }

    pub fn required_limit(&self) -> u32 {
        u32::from(self.words[self.count - 1].offset) + 1
    }

    /// stage read-after-save bytes only; this neither reads nor publishes memory.
    pub fn overlay(
        &self,
        outgoing_base: u32,
        incoming_base: u32,
        incoming: &mut [u8],
        authority: TaskSaveOverlayAuthority,
    ) -> Result<(), TaskSaveOverlayError> {
        if authority == TaskSaveOverlayAuthority::UnidentifiedPhysicalAliases {
            return Err(TaskSaveOverlayError::UnidentifiedPhysicalAliases);
        }
        if incoming.len() as u64 > (1_u64 << 32) {
            return Err(TaskSaveOverlayError::ImageTooLarge);
        }
        for word in self.words() {
            let address = outgoing_base.wrapping_add(u32::from(word.offset));
            for (byte, value) in word.value.to_le_bytes().into_iter().enumerate() {
                let offset = address
                    .wrapping_add(byte as u32)
                    .wrapping_sub(incoming_base) as usize;
                if let Some(destination) = incoming.get_mut(offset) {
                    *destination = value;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
