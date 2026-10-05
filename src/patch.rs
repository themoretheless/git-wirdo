use crate::model::FileEntry;

#[derive(Debug, Clone)]
pub struct Hunk {
    pub file: FileEntry,
    pub staged: bool,
    pub patch: Vec<u8>,
    pub snapshot: Vec<u8>,
}

/// Keep Git's exact bytes, including quoted paths and no-newline markers.
pub fn split(file: &FileEntry, staged: bool, diff: Vec<u8>) -> Vec<Hunk> {
    let lines: Vec<_> = diff.split_inclusive(|byte| *byte == b'\n').collect();
    let starts: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with(b"@@ "))
        .map(|(index, _)| index)
        .collect();
    let Some(first) = starts.first().copied() else {
        return Vec::new();
    };
    let header: Vec<u8> = lines[..first].concat();
    // A partial content patch must not implicitly rename a file or change its mode.
    if lines[..first].iter().any(|line| {
        line.starts_with(b"rename ")
            || line.starts_with(b"copy ")
            || line.starts_with(b"old mode ")
            || line.starts_with(b"new mode ")
    }) {
        return Vec::new();
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, start)| {
            let end = starts.get(i + 1).copied().unwrap_or(lines.len());
            let mut patch = header.clone();
            patch.extend_from_slice(&lines[*start..end].concat());
            Hunk {
                file: file.clone(),
                staged,
                patch,
                snapshot: diff.clone(),
            }
        })
        .collect()
}
