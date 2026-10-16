use crate::{
    process::HostError,
    windows::automation::{AutomationError32 as E, variants::Variant32},
};

#[derive(Clone, Copy, Default)]
struct Frame32 {
    pointer: u32,
    offset: usize,
}

pub(super) struct ReferenceCursor32 {
    root: bool,
    depth: usize,
    frames: [Frame32; 128],
}
impl Default for ReferenceCursor32 {
    fn default() -> Self {
        Self {
            root: false,
            depth: 0,
            frames: [Frame32::default(); 128],
        }
    }
}

impl ReferenceCursor32 {
    pub(super) fn next<'a>(
        &mut self,
        root: Variant32,
        nesting: usize,
        array: impl Fn(u32) -> Option<(u16, &'a [u8])>,
    ) -> Result<Option<u32>, HostError> {
        let mut value = if !self.root {
            self.root = true;
            Some(root)
        } else {
            None
        };
        loop {
            if let Some(value) = value.take() {
                if !value.by_ref() && value.pointer() != 0 {
                    if value.array() {
                        if self.depth >= nesting.min(self.frames.len())
                            || self.frames[..self.depth]
                                .iter()
                                .any(|frame| frame.pointer == value.pointer())
                        {
                            return Err(HostError::Automation(E::InvalidParameter));
                        }
                        self.frames[self.depth] = Frame32 {
                            pointer: value.pointer(),
                            offset: 0,
                        };
                        self.depth += 1;
                    } else if matches!(value.base(), 9 | 13) {
                        return Ok(Some(value.pointer()));
                    }
                }
            }
            if self.depth == 0 {
                return Ok(None);
            }
            let frame = &mut self.frames[self.depth - 1];
            let (kind, bytes) =
                array(frame.pointer).ok_or(HostError::Automation(E::InvalidParameter))?;
            if !matches!(kind, 9 | 12 | 13) || frame.offset == bytes.len() {
                self.depth -= 1;
                continue;
            }
            let size = if kind == 12 { 16 } else { 4 };
            let end = frame
                .offset
                .checked_add(size)
                .ok_or(HostError::Automation(E::Capacity))?;
            let payload = bytes
                .get(frame.offset..end)
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            frame.offset = end;
            value = Some(if kind == 12 {
                Variant32(payload.try_into().unwrap())
            } else {
                Variant32::new(kind, payload)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_array_cursor_walks_nested_references_in_element_order_without_element_rows() {
        let variants = [
            Variant32::new(13, &0x1234u32.to_le_bytes()),
            Variant32::new(0x400d, &0x5678u32.to_le_bytes()),
            Variant32::new(0x200d, &0x2000u32.to_le_bytes()),
            Variant32::new(9, &0x1234u32.to_le_bytes()),
        ]
        .into_iter()
        .flat_map(|value| value.0)
        .collect::<Vec<_>>();
        let mut pointers = vec![0; 4096 * 4];
        pointers[4095 * 4..].copy_from_slice(&0x5678u32.to_le_bytes());
        let arrays = [
            (0x1000, 12, variants.as_slice()),
            (0x2000, 13, pointers.as_slice()),
        ];
        let root = Variant32::new(0x200c, &0x1000u32.to_le_bytes());
        let mut cursor = ReferenceCursor32::default();
        let mut observed = Vec::new();
        while let Some(pointer) = cursor
            .next(root, 32, |pointer| {
                arrays
                    .iter()
                    .find(|row| row.0 == pointer)
                    .map(|row| (row.1, row.2))
            })
            .unwrap()
        {
            observed.push(pointer);
        }
        assert_eq!(observed, [0x1234, 0x5678, 0x1234]);
        assert_eq!(cursor.next(root, 32, |_| None).unwrap(), None);
        assert!(
            std::mem::size_of::<ReferenceCursor32>()
                <= 128 * 2 * std::mem::size_of::<usize>() + 2 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn frozen_cycles_and_nesting_overflow_do_not_loop_or_allocate_element_metadata() {
        let root = Variant32::new(0x200c, &0x1000u32.to_le_bytes());
        let bytes = root.0;
        let mut cursor = ReferenceCursor32::default();
        assert!(matches!(
            cursor.next(root, 32, |_| Some((12, bytes.as_slice()))),
            Err(HostError::Automation(E::InvalidParameter))
        ));
        assert!(matches!(
            ReferenceCursor32::default().next(root, 0, |_| None),
            Err(HostError::Automation(E::InvalidParameter))
        ));
    }
}
