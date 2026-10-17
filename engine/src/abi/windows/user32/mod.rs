use crate::{
    abi::{AbiError, header},
    process::User32WindowInfo32,
    windows::{
        user32::{Point32, Rect32},
        window::WindowExtent32,
    },
};

pub const USER32_WINDOW_INFO_SIZE: usize = 80;

pub fn encode_user32_window_info(
    info: Option<User32WindowInfo32>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != USER32_WINDOW_INFO_SIZE {
        return Err(AbiError::Length);
    }
    if let Some(info) = info {
        validate(info)?;
    }
    header::write(output, *b"R3UW");
    if let Some(info) = info {
        header::write_u32(output, 12, 1);
        for (index, word) in [
            info.window,
            info.parent,
            info.owner,
            info.thread,
            info.style,
            info.ex_style,
            info.outer.left as u32,
            info.outer.top as u32,
            info.outer.right as u32,
            info.outer.bottom as u32,
            info.client_origin.x as u32,
            info.client_origin.y as u32,
            info.client_extent.width,
            info.client_extent.height,
            u32::from(info.visible),
            u32::from(info.enabled),
        ]
        .into_iter()
        .enumerate()
        {
            header::write_u32(output, 16 + index * 4, word);
        }
    }
    Ok(())
}

pub fn decode_user32_window_info(input: &[u8]) -> Result<Option<User32WindowInfo32>, AbiError> {
    let (_, present) = header::validate_versions_with_metadata(
        input,
        *b"R3UW",
        USER32_WINDOW_INFO_SIZE,
        &[crate::abi::ABI_VERSION],
    )?;
    if present == 0 {
        return if input[16..].iter().any(|byte| *byte != 0) {
            Err(AbiError::Reserved)
        } else {
            Ok(None)
        };
    }
    if present != 1 || header::read_u32(input, 72) > 1 || header::read_u32(input, 76) > 1 {
        return Err(AbiError::Flags);
    }
    let info = User32WindowInfo32 {
        window: header::read_u32(input, 16),
        parent: header::read_u32(input, 20),
        owner: header::read_u32(input, 24),
        thread: header::read_u32(input, 28),
        style: header::read_u32(input, 32),
        ex_style: header::read_u32(input, 36),
        outer: Rect32 {
            left: header::read_u32(input, 40) as i32,
            top: header::read_u32(input, 44) as i32,
            right: header::read_u32(input, 48) as i32,
            bottom: header::read_u32(input, 52) as i32,
        },
        client_origin: Point32 {
            x: header::read_u32(input, 56) as i32,
            y: header::read_u32(input, 60) as i32,
        },
        client_extent: WindowExtent32 {
            width: header::read_u32(input, 64),
            height: header::read_u32(input, 68),
        },
        visible: header::read_u32(input, 72) == 1,
        enabled: header::read_u32(input, 76) == 1,
    };
    validate(info)?;
    Ok(Some(info))
}

fn validate(info: User32WindowInfo32) -> Result<(), AbiError> {
    let child = info.style & 0x4000_0000 != 0;
    if info.window == 0
        || info.thread == 0
        || (child && info.owner != 0)
        || (!child && info.parent != 0)
        || info.client_extent.width > i32::MAX as u32
        || info.client_extent.height > i32::MAX as u32
    {
        return Err(AbiError::Reserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
