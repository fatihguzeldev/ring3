use super::{
    BATCH_HEADER_SIZE, CodecError, FIRST_FRAGMENT, FRAGMENT_HEADER_SIZE, LAST_FRAGMENT, PROFILE,
    VERSION,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchHeader {
    pub length: u32,
    pub fragments: u32,
    pub epoch: u64,
    pub sequence: u64,
    pub first_command: u64,
    pub last_command: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragment<'a> {
    pub opcode: u16,
    pub flags: u16,
    pub command: u64,
    pub offset: u32,
    pub total: u32,
    pub bytes: &'a [u8],
}

pub fn write_batch_header(output: &mut [u8], header: BatchHeader) -> Result<(), CodecError> {
    if output.len() < BATCH_HEADER_SIZE || header.length < BATCH_HEADER_SIZE as u32 {
        return Err(CodecError::Length);
    }
    if header.epoch == 0
        || header.sequence == 0
        || header.first_command == 0
        || header.first_command > header.last_command
        || header.fragments == 0
    {
        return Err(CodecError::Identity);
    }
    output[..BATCH_HEADER_SIZE].fill(0);
    output[..4].copy_from_slice(b"R3D8");
    output[4..6].copy_from_slice(&VERSION.to_le_bytes());
    output[6..8].copy_from_slice(&PROFILE.to_le_bytes());
    output[8..12].copy_from_slice(&header.length.to_le_bytes());
    output[12..16].copy_from_slice(&header.fragments.to_le_bytes());
    output[16..24].copy_from_slice(&header.epoch.to_le_bytes());
    output[24..32].copy_from_slice(&header.sequence.to_le_bytes());
    output[32..40].copy_from_slice(&header.first_command.to_le_bytes());
    output[40..48].copy_from_slice(&header.last_command.to_le_bytes());
    Ok(())
}

pub fn append_fragment(output: &mut Vec<u8>, fragment: Fragment<'_>) -> Result<(), CodecError> {
    validate(fragment)?;
    let length = FRAGMENT_HEADER_SIZE
        .checked_add(fragment.bytes.len())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or(CodecError::Length)?;
    output
        .try_reserve(length as usize)
        .map_err(|_| CodecError::Allocation)?;
    output.extend_from_slice(&fragment.opcode.to_le_bytes());
    output.extend_from_slice(&fragment.flags.to_le_bytes());
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(&fragment.command.to_le_bytes());
    output.extend_from_slice(&fragment.offset.to_le_bytes());
    output.extend_from_slice(&fragment.total.to_le_bytes());
    output.extend_from_slice(&[0; 8]);
    output.extend_from_slice(fragment.bytes);
    Ok(())
}

pub fn packet(bytes: &[u8]) -> Result<(BatchHeader, FragmentIter<'_>), CodecError> {
    if bytes.len() < BATCH_HEADER_SIZE {
        return Err(CodecError::Length);
    }
    if &bytes[..4] != b"R3D8" {
        return Err(CodecError::Magic);
    }
    if u16_at(bytes, 4) != VERSION || u16_at(bytes, 6) != PROFILE {
        return Err(CodecError::Version);
    }
    if bytes[48..64].iter().any(|byte| *byte != 0) {
        return Err(CodecError::Reserved);
    }
    let header = BatchHeader {
        length: u32_at(bytes, 8),
        fragments: u32_at(bytes, 12),
        epoch: u64_at(bytes, 16),
        sequence: u64_at(bytes, 24),
        first_command: u64_at(bytes, 32),
        last_command: u64_at(bytes, 40),
    };
    if header.length as usize != bytes.len()
        || header.fragments == 0
        || header.fragments as usize > (bytes.len() - BATCH_HEADER_SIZE) / FRAGMENT_HEADER_SIZE
    {
        return Err(CodecError::Length);
    }
    if header.epoch == 0
        || header.sequence == 0
        || header.first_command == 0
        || header.first_command > header.last_command
    {
        return Err(CodecError::Identity);
    }
    let mut cursor = BATCH_HEADER_SIZE;
    let mut previous: Option<Fragment<'_>> = None;
    for index in 0..header.fragments {
        let (fragment, next) = fragment_at(bytes, cursor)?;
        if index == 0 && fragment.command != header.first_command {
            return Err(CodecError::Identity);
        }
        if let Some(previous) = previous {
            if fragment.command == previous.command {
                if previous.flags & LAST_FRAGMENT != 0
                    || fragment.flags & FIRST_FRAGMENT != 0
                    || previous.opcode != fragment.opcode
                    || previous.total != fragment.total
                    || previous.offset as usize + previous.bytes.len() != fragment.offset as usize
                {
                    return Err(CodecError::InvalidCommand);
                }
            } else if previous.command.checked_add(1) != Some(fragment.command)
                || previous.flags & LAST_FRAGMENT == 0
                || fragment.flags & FIRST_FRAGMENT == 0
            {
                return Err(CodecError::Identity);
            }
        }
        previous = Some(fragment);
        cursor = next;
    }
    if cursor != bytes.len() {
        return Err(CodecError::Length);
    }
    if previous.is_none_or(|fragment| fragment.command != header.last_command) {
        return Err(CodecError::Identity);
    }
    Ok((
        header,
        FragmentIter {
            bytes,
            cursor: BATCH_HEADER_SIZE,
            remaining: header.fragments,
        },
    ))
}

pub struct FragmentIter<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: u32,
}

impl<'a> Iterator for FragmentIter<'a> {
    type Item = Fragment<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let (fragment, next) = fragment_at(self.bytes, self.cursor).ok()?;
        self.cursor = next;
        self.remaining -= 1;
        Some(fragment)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining as usize, Some(self.remaining as usize))
    }
}

impl ExactSizeIterator for FragmentIter<'_> {}

fn fragment_at(bytes: &[u8], offset: usize) -> Result<(Fragment<'_>, usize), CodecError> {
    let header = bytes
        .get(
            offset
                ..offset
                    .checked_add(FRAGMENT_HEADER_SIZE)
                    .ok_or(CodecError::Length)?,
        )
        .ok_or(CodecError::Length)?;
    let length = u32_at(header, 4) as usize;
    if length < FRAGMENT_HEADER_SIZE {
        return Err(CodecError::Length);
    }
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or(CodecError::Length)?;
    if header[24..32].iter().any(|byte| *byte != 0) {
        return Err(CodecError::Reserved);
    }
    let fragment = Fragment {
        opcode: u16_at(header, 0),
        flags: u16_at(header, 2),
        command: u64_at(header, 8),
        offset: u32_at(header, 16),
        total: u32_at(header, 20),
        bytes: &bytes[offset + FRAGMENT_HEADER_SIZE..end],
    };
    validate(fragment)?;
    Ok((fragment, end))
}

fn validate(fragment: Fragment<'_>) -> Result<(), CodecError> {
    if !(1..=20).contains(&fragment.opcode)
        || fragment.flags & !(FIRST_FRAGMENT | LAST_FRAGMENT) != 0
    {
        return Err(CodecError::InvalidCommand);
    }
    if fragment.command == 0 {
        return Err(CodecError::Identity);
    }
    let end = (fragment.offset as usize)
        .checked_add(fragment.bytes.len())
        .ok_or(CodecError::Length)?;
    if end > fragment.total as usize
        || (fragment.flags & FIRST_FRAGMENT != 0) != (fragment.offset == 0)
        || (fragment.flags & LAST_FRAGMENT != 0) != (end == fragment.total as usize)
        || (fragment.bytes.is_empty() && fragment.total != 0)
    {
        return Err(CodecError::Length);
    }
    Ok(())
}

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
pub(super) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
pub(super) fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
