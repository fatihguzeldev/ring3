pub(crate) fn file_title(path: &[u16]) -> Option<&[u16]> {
    if path.is_empty()
        || path
            .iter()
            .any(|unit| matches!(unit, 0 | 34 | 42 | 60 | 62 | 63 | 124))
    {
        return None;
    }
    let start = path
        .iter()
        .rposition(|unit| matches!(unit, 47 | 92))
        .map_or_else(|| usize::from(path.get(1) == Some(&58)) * 2, |at| at + 1);
    let title = path.get(start..)?;
    (!title.is_empty() && !title.contains(&58)).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_path_titles_keep_extensions_and_drive_relative_names_without_reading_the_filesystem() {
        for path in [
            "C:\\directory\\file.txt",
            "C:file.txt",
            "file.txt",
            "\\\\server\\share\\file.txt",
        ] {
            let units = path.encode_utf16().collect::<Vec<_>>();
            assert_eq!(
                file_title(&units).unwrap(),
                "file.txt".encode_utf16().collect::<Vec<_>>()
            );
        }
        assert_eq!(
            file_title(&"C:\\directory\\".encode_utf16().collect::<Vec<_>>()),
            None
        );
    }
}
