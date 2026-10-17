use super::super::{memory, queries::metric_bytes};
use crate::windows::{
    gdi::{EnumeratedFont32, GdiError32},
    text::CodePage32,
};
fn text(output: &mut [u8], units: &[u16], wide: bool, cp: CodePage32) -> Result<(), GdiError32> {
    let mut bytes = memory::encode(units, wide, cp)?;
    let width = if wide { 2 } else { 1 };
    bytes.truncate(output.len() - width);
    if wide {
        if bytes.len() >= 2
            && (0xd800..=0xdbff).contains(&u16::from_le_bytes(
                bytes[bytes.len() - 2..].try_into().unwrap(),
            ))
        {
            bytes.truncate(bytes.len() - 2);
        }
    } else {
        while cp.decode(&bytes, true).is_err() {
            bytes.pop();
        }
    }
    output[..bytes.len()].copy_from_slice(&bytes);
    Ok(())
}
pub(super) fn payload(
    item: &EnumeratedFont32,
    wide: bool,
    cp: CodePage32,
) -> Result<(Vec<u8>, u32), GdiError32> {
    let mut bytes = memory::encode_font(item.font.descriptor(), wide, cp)?;
    let width = if wide { 2 } else { 1 };
    let start = bytes.len();
    bytes.resize(start + 128 * width, 0);
    text(
        &mut bytes[start..start + 64 * width],
        &item.full_name,
        wide,
        cp,
    )?;
    text(
        &mut bytes[start + 64 * width..start + 96 * width],
        &item.style,
        wide,
        cp,
    )?;
    text(
        &mut bytes[start + 96 * width..],
        &item.script.encode_utf16().collect::<Vec<_>>(),
        wide,
        cp,
    )?;
    let offset = bytes.len() as u32;
    let metrics = item.font.metrics()?;
    bytes.extend(metric_bytes(metrics, wide, cp)?);
    let flags = if metrics.italic { 1 } else { 0 }
        | if metrics.weight >= 700 { 32 } else { 0 }
        | if !metrics.italic && metrics.weight < 700 {
            64
        } else {
            0
        };
    for value in [
        flags,
        item.em,
        metrics.height as u32,
        metrics.average_width as u32,
    ]
    .into_iter()
    .chain(item.signature)
    {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok((bytes, offset))
}
