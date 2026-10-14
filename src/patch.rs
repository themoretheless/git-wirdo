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

impl Hunk {
    /// Select one-based changed-line ordinals, shown independently of context lines.
    pub fn select_lines(&self, selected: &[usize]) -> anyhow::Result<Self> {
        use anyhow::{Context, ensure};
        let lines: Vec<_> = self.patch.split_inclusive(|b| *b == b'\n').collect();
        let start = lines
            .iter()
            .position(|l| l.starts_with(b"@@ "))
            .context("Missing text hunk header")?;
        ensure!(
            !lines[..start]
                .iter()
                .any(|l| l.starts_with(b"new file mode") || l.starts_with(b"deleted file mode")),
            "Use whole-hunk staging for file creation or deletion"
        );
        let header = std::str::from_utf8(lines[start])?;
        let old = header
            .split_whitespace()
            .nth(1)
            .context("Missing old hunk range")?
            .trim_start_matches('-');
        let old_start: usize = old.split(',').next().unwrap().parse()?;
        let mut body = Vec::new();
        let (mut ordinal, mut old_count, mut new_count) = (0, 0, 0);
        let selected_set: std::collections::HashSet<_> = selected.iter().copied().collect();
        let mut keep_marker = false;
        for line in &lines[start + 1..] {
            let kind = *line.first().context("Empty hunk record")?;
            ensure!(
                matches!(kind, b' ' | b'+' | b'-' | b'\\'),
                "Invalid hunk record"
            );
            if kind == b'\\' {
                if keep_marker {
                    body.extend_from_slice(line);
                }
                continue;
            }
            let changed = kind == b'+' || kind == b'-';
            if changed {
                ordinal += 1;
            }
            let keep = selected_set.contains(&ordinal) && changed;
            let prefix = match (kind, keep, self.staged) {
                (b'+', false, false) | (b'-', false, true) => {
                    keep_marker = false;
                    continue;
                }
                (b'-', false, false) | (b'+', false, true) => b' ',
                _ => kind,
            };
            old_count += usize::from(prefix != b'+');
            new_count += usize::from(prefix != b'-');
            body.push(prefix);
            body.extend_from_slice(&line[1..]);
            keep_marker = true;
        }
        ensure!(
            !selected.is_empty() && selected.iter().all(|n| *n > 0 && *n <= ordinal),
            "Select existing changed-line numbers"
        );
        // Reverse application uses the original new-side location. Earlier hunks
        // retain their original offsets, which Git adjusts when applying this hunk.
        let new: usize = header
            .split_whitespace()
            .nth(2)
            .context("Missing new hunk range")?
            .trim_start_matches('+')
            .split(',')
            .next()
            .unwrap()
            .parse()?;
        let new_start = if self.staged {
            new
        } else if old_count == 0 {
            old_start + 1
        } else if new_count == 0 {
            old_start.saturating_sub(1)
        } else {
            old_start
        };
        let old_start = if self.staged {
            if new_count == 0 {
                new + 1
            } else if old_count == 0 {
                new.saturating_sub(1)
            } else {
                new
            }
        } else {
            old_start
        };
        let mut patch = lines[..start].concat();
        patch.extend_from_slice(
            format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n").as_bytes(),
        );
        patch.extend(body);
        let mut result = self.clone();
        result.patch = patch;
        Ok(result)
    }

    pub fn numbered_changes(&self) -> String {
        let mut n = 0;
        let mut body = false;
        String::from_utf8_lossy(&self.patch)
            .lines()
            .map(|line| {
                if line.starts_with("@@ ") {
                    body = true;
                }
                if body && (line.starts_with('+') || line.starts_with('-')) {
                    n += 1;
                    format!("{n:>4} {line}")
                } else {
                    format!("     {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
