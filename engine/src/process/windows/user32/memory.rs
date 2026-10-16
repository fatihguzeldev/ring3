use crate::{
    memory::{AddressSpace, GuestAddress, PageRange},
    windows::user32::{Message32, Point32, Rect32, User32Error32},
};

#[derive(Clone, Copy)]
pub(super) struct Scratch32 {
    pub address: GuestAddress,
    lease: super::super::callbacks::GuestCallbackScratch32,
}

impl Scratch32 {
    pub(in crate::process::windows::user32) fn range(self) -> PageRange {
        self.lease.range()
    }

    pub(super) fn allocate(
        memory: &mut AddressSpace,
        bounds: PageRange,
        bytes: &[u8],
    ) -> Result<Self, User32Error32> {
        let lease =
            super::super::callbacks::GuestCallbackScratch32::allocate(memory, bounds, bytes)
                .map_err(scratch_error)?;
        Ok(Self {
            address: lease.address,
            lease,
        })
    }

    pub(super) fn check(self, memory: &AddressSpace) -> Result<(), User32Error32> {
        self.lease.check(memory).map_err(scratch_error)
    }

    pub(super) fn release(self, memory: &mut AddressSpace) -> Result<(), User32Error32> {
        self.lease.release(memory).map_err(scratch_error)
    }
}

fn scratch_error(error: crate::memory::MemoryError) -> User32Error32 {
    if error == crate::memory::MemoryError::InvalidRange {
        User32Error32::InvalidRequest
    } else {
        User32Error32::Memory(error)
    }
}

pub(super) fn words<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> Result<[u32; N], User32Error32> {
    let mut values = [0; N];
    for (index, value) in values.iter_mut().enumerate() {
        let mut bytes = [0; 4];
        memory.read(
            GuestAddress(
                address
                    .checked_add(index as u32 * 4)
                    .ok_or(User32Error32::Win32(87))?,
            ),
            &mut bytes,
        )?;
        *value = u32::from_le_bytes(bytes);
    }
    Ok(values)
}

pub(super) fn string(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    limit: usize,
    code_page: crate::windows::text::CodePage32,
) -> Result<Vec<u16>, User32Error32> {
    if address == 0 {
        return Err(User32Error32::Win32(87));
    }
    let byte_limit = limit
        .checked_mul(if wide {
            2
        } else {
            code_page.maximum_char_size() as usize
        })
        .ok_or(User32Error32::Win32(87))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_limit.min(512))
        .map_err(|_| User32Error32::Allocation)?;
    let step = if wide { 2 } else { 1 };
    for offset in (0..=byte_limit).step_by(step) {
        let at = address
            .checked_add(u32::try_from(offset).map_err(|_| User32Error32::Win32(87))?)
            .ok_or(User32Error32::Win32(87))?;
        let mut unit = [0; 2];
        memory.read(GuestAddress(at), &mut unit[..step])?;
        if unit[..step].iter().all(|&value| value == 0) {
            let output = if wide {
                bytes
                    .chunks_exact(2)
                    .map(|value| u16::from_le_bytes([value[0], value[1]]))
                    .collect()
            } else {
                code_page
                    .decode(&bytes, false)
                    .map_err(|_| User32Error32::Win32(1113))?
            };
            return if output.len() <= limit {
                Ok(output)
            } else {
                Err(User32Error32::Win32(87))
            };
        }
        if offset == byte_limit {
            return Err(User32Error32::Win32(87));
        }
        bytes
            .try_reserve(step)
            .map_err(|_| User32Error32::Allocation)?;
        bytes.extend_from_slice(&unit[..step]);
    }
    unreachable!()
}

pub(super) fn rect(memory: &AddressSpace, address: u32) -> Result<Rect32, User32Error32> {
    let [left, top, right, bottom] = words(memory, address)?;
    Ok(Rect32 {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}

pub(super) fn write_rect(
    memory: &mut AddressSpace,
    address: u32,
    rect: Rect32,
) -> Result<(), User32Error32> {
    let mut bytes = [0; 16];
    for (index, value) in [rect.left, rect.top, rect.right, rect.bottom]
        .into_iter()
        .enumerate()
    {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    memory.write(GuestAddress(address), &bytes)?;
    Ok(())
}

pub(super) fn message(memory: &AddressSpace, address: u32) -> Result<Message32, User32Error32> {
    let [window, message, wparam, lparam, time_ms, x, y] = words(memory, address)?;
    Ok(Message32 {
        window,
        message,
        wparam,
        lparam,
        time_ms,
        point: Point32 {
            x: x as i32,
            y: y as i32,
        },
        thread_id: 0,
    })
}

pub(super) fn write_message(
    memory: &mut AddressSpace,
    address: u32,
    message: Message32,
) -> Result<(), User32Error32> {
    let mut bytes = [0; 28];
    for (index, value) in [
        message.window,
        message.message,
        message.wparam,
        message.lparam,
        message.time_ms,
        message.point.x as u32,
        message.point.y as u32,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    memory.write(GuestAddress(address), &bytes)?;
    Ok(())
}
