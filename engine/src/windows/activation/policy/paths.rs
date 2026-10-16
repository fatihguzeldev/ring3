use crate::windows::activation::ActivationError32 as E;

pub(super) fn private_paths(value: &str) -> Result<Vec<Vec<u16>>, E> {
    let mut paths = Vec::new();
    for path in value.split(';').filter(|path| !path.is_empty()) {
        if paths.len() == 9
            || path.starts_with(['/', '\\'])
            || path.contains(':')
            || path.chars().any(char::is_control)
        {
            return Err(E::ManifestFormat);
        }
        let mut depth = 0i32;
        for part in path.split(['/', '\\']) {
            match part {
                ".." => depth -= 1,
                "." | "" => {}
                "..." => return Err(E::ManifestFormat),
                _ if part.contains(['*', '?', '"', '<', '>', '|']) => {
                    return Err(E::ManifestFormat);
                }
                _ => depth += 1,
            }
            if depth < -2 {
                return Err(E::ManifestFormat);
            }
        }
        paths.push(
            path.encode_utf16()
                .map(|unit| {
                    if unit == u16::from(b'/') {
                        u16::from(b'\\')
                    } else {
                        unit
                    }
                })
                .collect(),
        );
    }
    Ok(paths)
}
