use crate::files::{FileAccess32, FileError32};

pub(in crate::process::windows::files) fn desired_access(
    raw: u32,
) -> Result<FileAccess32, FileError32> {
    if raw & !0xf21f_01ff != 0 {
        return Err(FileError32::InvalidParameter);
    }
    if raw & 0x1000_0000 != 0 {
        return Ok(FileAccess32::ALL);
    }
    let mut access = 0;
    if raw & (0x8000_0000 | 1) != 0 {
        access |= FileAccess32::READ.0;
    }
    if raw & (0x4000_0000 | 2) != 0 {
        access |= FileAccess32::WRITE.0;
    }
    if raw & (0x4000_0000 | 4) != 0 {
        access |= FileAccess32::APPEND.0;
    }
    if raw & (0x2000_0000 | 0x20) != 0 {
        access |= FileAccess32::EXECUTE.0;
    }
    if raw & 0x0001_0000 != 0 {
        access |= FileAccess32::DELETE.0;
    }
    if raw & (0xa000_0000 | 0x80) != 0 {
        access |= FileAccess32::READ_ATTRIBUTES.0;
    }
    if raw & (0x4000_0000 | 0x100) != 0 {
        access |= FileAccess32::WRITE_ATTRIBUTES.0;
    }
    if raw & (0xf000_0000 | 0x0002_0000) != 0 {
        access |= FileAccess32::READ_CONTROL.0;
    }
    if raw & 0x0004_0000 != 0 {
        access |= FileAccess32::WRITE_DAC.0;
    }
    if raw & 0x0008_0000 != 0 {
        access |= FileAccess32::WRITE_OWNER.0;
    }
    Ok(FileAccess32(access))
}

pub(in crate::process::windows::files) fn desired_handle_access(
    raw: u32,
) -> Result<u32, FileError32> {
    Ok(desired_access(raw)?.0
        | if raw & (0xf000_0000 | 0x0010_0000) != 0 {
            0x0010_0000
        } else {
            0
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_file_generic_masks_preserve_data_and_attribute_rights() {
        assert_eq!(desired_access(0x0012_0089), Ok(FileAccess32(145)));
        assert_eq!(desired_access(0x0012_0116), Ok(FileAccess32(166)));
        assert_eq!(desired_access(0x001f_01ff), Ok(FileAccess32::ALL));
        assert_eq!(desired_access(0), Ok(FileAccess32(0)));
        assert_eq!(desired_handle_access(0x8000_0000), Ok(0x0010_0091));
        assert_eq!(desired_handle_access(0x2000_0000), Ok(0x0010_00d0));
        assert_eq!(desired_access(0x0004_0000), Ok(FileAccess32::WRITE_DAC));
        assert_eq!(desired_access(0x0008_0000), Ok(FileAccess32::WRITE_OWNER));
        assert_eq!(desired_access(0x0200_0000), Ok(FileAccess32(0)));
        assert_eq!(desired_handle_access(1), Ok(1));
        assert_eq!(desired_handle_access(0), Ok(0));
    }
}
