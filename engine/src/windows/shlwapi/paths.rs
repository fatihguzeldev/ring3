use super::ShlwapiError32 as E;

pub(crate) const MAX_PATH: usize = 260;
const SLASH: u16 = b'\\' as u16;

pub(crate) fn root_length(path: &[u16]) -> usize {
    if path.starts_with(&[SLASH, SLASH]) {
        let mut separators = path[2..].iter().enumerate().filter(|&(_, c)| *c == SLASH);
        let Some((server_end, _)) = separators.next() else {
            return path.len();
        };
        return separators.next().map_or(path.len(), |(share_end, _)| {
            if share_end > server_end + 1 {
                share_end + 2
            } else {
                0
            }
        });
    }
    if path.first() == Some(&SLASH) {
        return 1;
    }
    if path.len() >= 2 && path[1] == b':' as u16 {
        return if path.get(2) == Some(&SLASH) { 3 } else { 2 };
    }
    0
}

pub(crate) fn is_relative(path: &[u16]) -> bool {
    path.first() != Some(&SLASH) && path.get(1) != Some(&(b':' as u16))
}
pub(crate) fn is_root(path: &[u16]) -> bool {
    !path.is_empty()
        && path.len() == root_length(path)
        && !(path.len() == 2 && path[1] == b':' as u16)
}
pub(crate) fn file_name(path: &[u16]) -> usize {
    let mut start = 0;
    for (index, pair) in path.windows(2).enumerate() {
        if matches!(pair[0], 92 | 47 | 58) && !matches!(pair[1], 92 | 47) {
            start = index + 1;
        }
    }
    start
}
pub(crate) fn extension(path: &[u16]) -> usize {
    let mut extension = path.len();
    for (index, unit) in path.iter().copied().enumerate() {
        match unit {
            92 | 47 | 32 => extension = path.len(),
            46 => extension = index,
            _ => {}
        }
    }
    extension
}
pub(crate) fn remove_file_spec(path: &[u16]) -> usize {
    let drive = path.get(1) == Some(&(b':' as u16));
    match path.iter().rposition(|&c| c == SLASH) {
        Some(2) if drive => 3,
        Some(0) => 1,
        Some(at) if path.starts_with(&[SLASH, SLASH]) && at == 1 => 2,
        Some(at) => at,
        None if drive => 2,
        None => 0,
    }
    .min(path.len())
}

pub(crate) fn canonicalize(path: &[u16]) -> Result<Vec<u16>, E> {
    if path.len() >= MAX_PATH {
        return Err(E::FilenameTooLong);
    }
    let root = root_length(path);
    let mut out = Vec::new();
    out.try_reserve_exact(MAX_PATH).map_err(|_| E::Allocation)?;
    out.extend_from_slice(&path[..root]);
    for component in path[root..].split(|&c| c == SLASH) {
        if component.is_empty() || component == [b'.' as u16] {
            continue;
        }
        if component == [b'.' as u16, b'.' as u16] {
            if out.len() > root {
                let end = out
                    .iter()
                    .rposition(|&c| c == SLASH)
                    .unwrap_or(root)
                    .max(root);
                out.truncate(end);
                if out.is_empty() {
                    out.push(SLASH);
                }
            }
            continue;
        }
        if !out.is_empty()
            && out.last() != Some(&SLASH)
            && !(root == 2 && out.len() == root && path.get(1) == Some(&(b':' as u16)))
        {
            out.push(SLASH);
        }
        out.extend_from_slice(component);
    }
    if path.last() == Some(&SLASH) && !out.is_empty() && out.last() != Some(&SLASH) {
        out.push(SLASH);
    }
    if out.len() == 2 && out[1] == b':' as u16 {
        out.push(SLASH);
    }
    if out.is_empty() {
        out.push(SLASH);
    }
    Ok(out)
}

pub(crate) fn combine(directory: Option<&[u16]>, file: Option<&[u16]>) -> Result<Vec<u16>, E> {
    let (directory, file) = match (directory, file) {
        (None, None) => return Err(E::InvalidParameter),
        (directory, file) => (directory.unwrap_or_default(), file.unwrap_or_default()),
    };
    if file.len() >= MAX_PATH || directory.len() >= MAX_PATH {
        return Err(E::FilenameTooLong);
    }
    if file.is_empty() {
        return canonicalize(directory);
    }
    if !is_relative(file) && file.first() != Some(&SLASH) || file.starts_with(&[SLASH, SLASH]) {
        return canonicalize(file);
    }
    let mut joined = Vec::new();
    joined
        .try_reserve_exact(MAX_PATH * 2)
        .map_err(|_| E::Allocation)?;
    let directory = if file.first() == Some(&SLASH) {
        &directory[..root_length(directory)]
    } else {
        directory
    };
    joined.extend_from_slice(directory);
    if !joined.is_empty() && joined.last() != Some(&SLASH) {
        joined.push(SLASH);
    }
    joined.extend_from_slice(if file.first() == Some(&SLASH) && !joined.is_empty() {
        &file[1..]
    } else {
        file
    });
    if joined.len() >= MAX_PATH {
        return Err(E::FilenameTooLong);
    }
    canonicalize(&joined)
}

pub(crate) fn append(directory: &[u16], file: &[u16]) -> Result<Vec<u16>, E> {
    // PathAppend treats one leading slash as a relative suffix; UNC remains absolute.
    let file = if file.first() == Some(&SLASH) && file.get(1) != Some(&SLASH) {
        &file[1..]
    } else {
        file
    };
    combine(Some(directory), Some(file))
}
