use crate::abi::{AbiError, header};

pub const WINDOWS_DIALOG_HEADER_SIZE: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsDialogIcon32<'a> {
    Standard(u32),
    Resource { instance: u32, id: u16 },
    NamedResource { instance: u32, name: &'a [u16] },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsDialogRecord32<'a> {
    pub epoch: u64,
    pub id: u64,
    pub thread: u32,
    pub owner: u32,
    pub style: u32,
    pub language: u16,
    pub help_context: u32,
    pub caption: &'a [u16],
    pub text: &'a [u16],
    pub icon: WindowsDialogIcon32<'a>,
}

pub fn encode_windows_dialog_page(
    request: Option<WindowsDialogRecord32<'_>>,
    field: u32,
    offset: u32,
    maximum: u32,
    output: &mut [u8],
) -> Result<usize, AbiError> {
    if output.len() < WINDOWS_DIALOG_HEADER_SIZE
        || maximum as usize > (output.len() - WINDOWS_DIALOG_HEADER_SIZE) / 2
    {
        return Err(AbiError::Length);
    }
    let Some(request) = request else {
        if field != 0 || offset != 0 || maximum != 0 {
            return Err(AbiError::Reserved);
        }
        header::write(&mut output[..WINDOWS_DIALOG_HEADER_SIZE], *b"R3DG");
        return Ok(WINDOWS_DIALOG_HEADER_SIZE);
    };
    if request.epoch == 0 || request.id == 0 || request.thread == 0 {
        return Err(AbiError::Reserved);
    }
    let (icon_kind, icon_instance, icon_value, icon_name) = match request.icon {
        WindowsDialogIcon32::Standard(value) => (0, 0, value, &[][..]),
        WindowsDialogIcon32::Resource { instance, id } => (1, instance, u32::from(id), &[][..]),
        WindowsDialogIcon32::NamedResource { instance, name } => (2, instance, 0, name),
    };
    let units = match field {
        0 if offset == 0 && maximum == 0 => &[][..],
        1 => request.caption,
        2 => request.text,
        3 => icon_name,
        _ => return Err(AbiError::Reserved),
    };
    let start = offset as usize;
    let remaining = units.get(start..).ok_or(AbiError::Length)?;
    let page = &remaining[..remaining.len().min(maximum as usize)];
    let counts = [request.caption.len(), request.text.len(), icon_name.len()].map(u32::try_from);
    let [caption, text, icon] = counts;
    let counts = [
        caption.map_err(|_| AbiError::Length)?,
        text.map_err(|_| AbiError::Length)?,
        icon.map_err(|_| AbiError::Length)?,
    ];
    let length = WINDOWS_DIALOG_HEADER_SIZE + page.len() * 2;
    header::write(&mut output[..length], *b"R3DG");
    header::write_u32(output, 12, 1);
    output[16..24].copy_from_slice(&request.epoch.to_le_bytes());
    output[24..32].copy_from_slice(&request.id.to_le_bytes());
    for (at, value) in [
        (32, request.thread),
        (36, request.owner),
        (40, request.style),
        (44, u32::from(request.language)),
        (48, request.help_context),
        (52, icon_kind),
        (56, icon_instance),
        (60, icon_value),
        (64, counts[0]),
        (68, counts[1]),
        (72, counts[2]),
        (76, field),
        (80, offset),
        (84, page.len() as u32),
    ] {
        header::write_u32(output, at, value);
    }
    for (chunk, unit) in output[WINDOWS_DIALOG_HEADER_SIZE..length]
        .chunks_exact_mut(2)
        .zip(page)
    {
        chunk.copy_from_slice(&unit.to_le_bytes());
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_pages_preserve_literal_identity_offsets_unicode_and_failure_neutrality() {
        let request = WindowsDialogRecord32 {
            epoch: 0x100000002,
            id: 0x300000004,
            thread: 2,
            owner: 0x1234,
            style: 0x4003,
            language: 0x411,
            help_context: 0x11223344,
            caption: &[0x65e5, 0x672c],
            text: &[0x41, 0xd83d, 0xde42, 0x42],
            icon: WindowsDialogIcon32::NamedResource {
                instance: 0x400000,
                name: &[0x58],
            },
        };
        let mut bytes = [0xab; 104];
        let length = encode_windows_dialog_page(Some(request), 2, 1, 2, &mut bytes).unwrap();
        assert_eq!(length, 100);
        assert_eq!(
            &bytes[..16],
            &[82, 51, 68, 71, 1, 0, 1, 0, 100, 0, 0, 0, 1, 0, 0, 0]
        );
        assert_eq!(
            &bytes[16..32],
            &[2, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 3, 0, 0, 0]
        );
        assert_eq!(
            &bytes[64..88],
            &[
                2, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0
            ]
        );
        assert_eq!(
            &bytes[88..100],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0x3d, 0xd8, 0x42, 0xde]
        );
        let before = bytes;
        assert_eq!(
            encode_windows_dialog_page(Some(request), 2, 5, 1, &mut bytes),
            Err(AbiError::Length)
        );
        assert_eq!(bytes, before);
        assert_eq!(
            encode_windows_dialog_page(None, 0, 0, 0, &mut bytes).unwrap(),
            96
        );
        assert!(bytes[12..96].iter().all(|byte| *byte == 0));
    }
}
