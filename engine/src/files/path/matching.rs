use super::fold;

pub(crate) fn matches(pattern: &[u16], name: &[u16]) -> bool {
    let pattern = fold(pattern);
    let name = fold(name);
    let mut tokens = Vec::with_capacity(pattern.len());
    if pattern == [42, 46, 42] {
        tokens.push(42);
    } else {
        for (index, &unit) in pattern.iter().enumerate() {
            match unit {
                63 => tokens.push(62),
                46 if index + 1 == pattern.len() && tokens.last() == Some(&42) => {
                    *tokens.last_mut().unwrap() = 60;
                }
                46 if matches!(pattern.get(index + 1), Some(42 | 63)) => tokens.push(34),
                _ => tokens.push(unit),
            }
        }
    }
    let last_dot = name.iter().rposition(|u| *u == 46);
    let mut suffix = vec![false; name.len() + 1];
    suffix[name.len()] = true;
    for &token in tokens.iter().rev() {
        let mut row = vec![false; name.len() + 1];
        for position in (0..=name.len()).rev() {
            let current = name.get(position).copied();
            row[position] = match token {
                42 => suffix[position] || (position < name.len() && row[position + 1]),
                60 => {
                    suffix[position]
                        || (position < name.len()
                            && Some(position) != last_dot
                            && row[position + 1])
                }
                62 if current.is_none() || current == Some(46) => suffix[position],
                62 => suffix[position + 1],
                34 if current.is_none() => suffix[position],
                34 => current == Some(46) && suffix[position + 1],
                _ => current == Some(token) && suffix[position + 1],
            };
        }
        suffix = row;
    }
    suffix[0]
}
