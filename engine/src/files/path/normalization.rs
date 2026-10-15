use crate::files::FileError32;

const SEP: u16 = b'\\' as u16;

pub(crate) fn fold(path: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(path.len());
    for scalar in char::decode_utf16(path.iter().copied()) {
        match scalar {
            Ok(ch) => {
                let mut upper = ch.to_uppercase();
                let first = upper.next().unwrap_or(ch);
                let chosen = if upper.next().is_none() { first } else { ch };
                let mut units = [0; 2];
                out.extend_from_slice(chosen.encode_utf16(&mut units));
            }
            Err(unpaired) => out.push(unpaired.unpaired_surrogate()),
        }
    }
    out
}

pub(crate) fn root_len(path: &[u16]) -> Result<usize, FileError32> {
    if path.len() >= 3 && path[1] == b':' as u16 && path[2] == SEP {
        return Ok(3);
    }
    if path.starts_with(&[SEP, SEP]) {
        let server = path[2..]
            .iter()
            .position(|u| *u == SEP)
            .ok_or(FileError32::InvalidName)?
            + 2;
        let share = path[server + 1..]
            .iter()
            .position(|u| *u == SEP)
            .map(|n| n + server + 1)
            .unwrap_or(path.len());
        if server == 2 || share == server + 1 {
            return Err(FileError32::InvalidName);
        }
        return Ok((share + 1).min(path.len()));
    }
    Err(FileError32::InvalidName)
}

pub(crate) fn normalize(
    input: &[u16],
    cwd: &[u16],
    wildcard: bool,
) -> Result<Vec<u16>, FileError32> {
    if input.is_empty() || input.contains(&0) {
        return Err(FileError32::InvalidName);
    }
    let extended = input.starts_with(&[SEP, SEP, b'?' as u16, SEP]);
    if input.starts_with(&[SEP, SEP, b'.' as u16, SEP]) {
        return Err(FileError32::Unsupported);
    }
    let mut raw = if extended {
        input[4..].to_vec()
    } else {
        input.to_vec()
    };
    if extended && raw.starts_with(&[b'U' as u16, b'N' as u16, b'C' as u16, SEP]) {
        raw.splice(..4, [SEP, SEP]);
    }
    if !extended {
        for unit in &mut raw {
            if *unit == b'/' as u16 {
                *unit = SEP;
            }
        }
    }
    let absolute = if raw.len() >= 2 && raw[1] == b':' as u16 {
        if !((b'A' as u16..=b'Z' as u16).contains(&raw[0])
            || (b'a' as u16..=b'z' as u16).contains(&raw[0]))
        {
            return Err(FileError32::InvalidName);
        }
        raw[0] = (raw[0] as u8).to_ascii_uppercase() as u16;
        if raw.get(2) == Some(&SEP) {
            raw
        } else {
            let mut base = if cwd.get(0).copied() == Some(raw[0]) {
                cwd.to_vec()
            } else {
                vec![raw[0], b':' as u16, SEP]
            };
            if base.last() != Some(&SEP) {
                base.push(SEP);
            }
            base.extend_from_slice(&raw[2..]);
            base
        }
    } else if raw.starts_with(&[SEP, SEP]) {
        raw
    } else if raw.first() == Some(&SEP) {
        let mut base = cwd[..root_len(cwd)?].to_vec();
        base.extend_from_slice(&raw[1..]);
        base
    } else {
        let mut base = cwd.to_vec();
        if base.last() != Some(&SEP) {
            base.push(SEP);
        }
        base.extend_from_slice(&raw);
        base
    };
    let root = root_len(&absolute)?;
    let mut out = absolute[..root].to_vec();
    if out.last() != Some(&SEP) {
        out.push(SEP);
    }
    let root = out.len();
    let pieces: Vec<&[u16]> = absolute[root.min(absolute.len())..]
        .split(|u| *u == SEP)
        .collect();
    for (index, piece) in pieces.iter().enumerate() {
        let mut part = *piece;
        if part.is_empty() {
            continue;
        }
        if !extended {
            if part == [b'.' as u16] {
                continue;
            }
            if part == [b'.' as u16, b'.' as u16] {
                if out.len() > root {
                    let end = out.iter().rposition(|u| *u == SEP).unwrap_or(root - 1);
                    out.truncate(end.max(root));
                }
                continue;
            }
            while matches!(part.last(), Some(32 | 46))
                && !(wildcard
                    && index + 1 == pieces.len()
                    && part.iter().any(|u| [42, 63].contains(u)))
            {
                part = &part[..part.len() - 1];
            }
            if part.is_empty() {
                return Err(FileError32::InvalidName);
            }
        } else if part == [46] || part == [46, 46] {
            return Err(FileError32::InvalidName);
        }
        if part.len() > 255 {
            return Err(FileError32::InvalidName);
        }
        for &unit in part {
            if unit < 32
                || [
                    b'<' as u16,
                    b'>' as u16,
                    b'"' as u16,
                    b'|' as u16,
                    b'/' as u16,
                ]
                .contains(&unit)
            {
                return Err(FileError32::InvalidName);
            }
            if unit == b':' as u16 {
                return Err(FileError32::Unsupported);
            }
            if [b'*' as u16, b'?' as u16].contains(&unit)
                && !(wildcard && index + 1 == pieces.len())
            {
                return Err(FileError32::InvalidName);
            }
        }
        if reserved(part) {
            return Err(FileError32::Unsupported);
        }
        if out.len() > root {
            out.push(SEP);
        }
        out.extend_from_slice(part);
    }
    if out.len() > if extended { 32767 } else { 259 } {
        return Err(FileError32::InvalidName);
    }
    Ok(out)
}

fn reserved(part: &[u16]) -> bool {
    let stem = fold(part.split(|u| *u == 46).next().unwrap_or(part));
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|s| stem == s.encode_utf16().collect::<Vec<_>>())
    {
        return true;
    }
    stem.len() == 4
        && (&stem[..3] == [67, 79, 77] || &stem[..3] == [76, 80, 84])
        && ([49, 50, 51, 52, 53, 54, 55, 56, 57, 185, 178, 179].contains(&stem[3]))
}

pub(crate) fn parent(path: &[u16]) -> Result<&[u16], FileError32> {
    let root = root_len(path)?;
    if path.len() <= root {
        return Err(FileError32::AccessDenied);
    }
    let index = path
        .iter()
        .rposition(|u| *u == SEP)
        .ok_or(FileError32::InvalidName)?;
    Ok(&path[..index.max(root)])
}

pub(crate) fn name(path: &[u16]) -> &[u16] {
    let index = path.iter().rposition(|u| *u == SEP).map_or(0, |n| n + 1);
    &path[index..]
}
