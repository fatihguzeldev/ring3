use super::{MessageError32 as E, append};

pub(super) fn wrap(source: &[u16], width: u8) -> Result<Vec<u16>, E> {
    if width == 0 || width == 255 {
        let mut result = Vec::new();
        append(&mut result, source)?;
        return Ok(result);
    }
    let width = usize::from(width);
    let mut result = Vec::new();
    let mut line = Vec::new();
    line.try_reserve_exact(width + 1)
        .map_err(|_| E::Allocation)?;
    for &unit in source {
        if unit == 13 || unit == 10 {
            append(&mut result, &line)?;
            line.clear();
            append(&mut result, &[unit])?;
            continue;
        }
        line.push(unit);
        if line.len() > width {
            if let Some(space) = line.iter().rposition(|&unit| unit == 32 || unit == 9) {
                if space > 0 {
                    append(&mut result, &line[..space])?;
                    append(&mut result, &[13, 10])?;
                    line.drain(..=space);
                    continue;
                }
            }
            append(&mut result, &line[..width])?;
            append(&mut result, &[13, 10])?;
            line.drain(..width);
        }
    }
    append(&mut result, &line)?;
    Ok(result)
}
