/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use unidiff::{Hunk, Line, PatchedFile};

/// Number of lines before and after the first modified and last modified lines in a diff that will
/// be displayed by default. Lines outside of this range will be hidden by default (but can be shown
/// by clicking a link to expand the context).
const DISPLAYED_CONTEXT_LINES: usize = 10;

#[derive(Debug, Default)]
pub struct PageTracker {
    max_files_per_page: usize,
    max_lines_per_page: usize,
    files: usize,
    lines: usize,
}

impl PageTracker {
    pub fn new(max_files_per_page: usize, max_lines_per_page: usize) -> Self {
        Self {
            max_files_per_page,
            max_lines_per_page,
            files: 0,
            lines: 0,
        }
    }

    pub fn next_file_is_page_boundary(&mut self) -> bool {
        self.files += 1;
        self.files > self.max_files_per_page || self.lines >= self.max_lines_per_page
    }

    pub fn total_modified_lines(&mut self, lines: usize) {
        self.lines += lines;
    }

    pub fn reset(&mut self) {
        self.files = 0;
        self.lines = 0;
    }
}

#[derive(Debug, Default)]
pub struct Page {
    pub files: Vec<File>,
}

#[derive(Debug)]
pub enum File {
    New {
        name: String,
        sections: Vec<Section>,
    },
    Removed {
        name: String,
        sections: Vec<Section>,
    },
    Modified {
        old_name: String,
        new_name: String,
        sections: Vec<Section>,
    },
}

impl File {
    pub fn name(&self) -> &str {
        match self {
            Self::New { name, .. } => name,
            Self::Removed { name, .. } => name,
            Self::Modified { new_name, .. } => new_name,
        }
    }
    pub fn sections(&self) -> &[Section] {
        match self {
            Self::New { sections, .. } => sections.as_ref(),
            Self::Removed { sections, .. } => sections.as_ref(),
            Self::Modified { sections, .. } => sections.as_ref(),
        }
    }
}

impl From<PatchedFile> for File {
    fn from(patched_file: PatchedFile) -> Self {
        let sections = patched_file.hunks().iter().map(Section::from).collect();
        let source = patched_file
            .source_file
            .strip_prefix("a/")
            .unwrap_or(&patched_file.source_file);
        let target = patched_file
            .target_file
            .strip_prefix("b/")
            .unwrap_or(&patched_file.target_file);
        if source == "/dev/null" {
            File::New {
                name: target.into(),
                sections,
            }
        } else if target == "/dev/null" {
            File::Removed {
                name: source.into(),
                sections,
            }
        } else {
            File::Modified {
                old_name: source.into(),
                new_name: target.into(),
                sections,
            }
        }
    }
}

#[derive(Debug)]
pub struct Section {
    pub start_line: (usize, usize),
    pub context_prefix: Option<Vec<Line>>,
    pub diff: Vec<Line>,
    pub context_suffix: Option<Vec<Line>>,
    pub end_line: (usize, usize),
}

impl From<&Hunk> for Section {
    fn from(hunk: &Hunk) -> Self {
        assert!(!hunk.lines().is_empty());
        let mut diff_start = None;
        let mut suffix_start = None;
        for (index, line) in hunk.lines().iter().enumerate() {
            if diff_start.is_none() {
                if line.line_type != " " {
                    diff_start = Some(index);
                }
            } else if suffix_start.is_some() && line.line_type != " " {
                suffix_start = None;
            } else if suffix_start.is_none() && line.line_type == " " {
                suffix_start = Some(index);
            }
        }

        let diff_start = diff_start.unwrap().saturating_sub(DISPLAYED_CONTEXT_LINES);
        let suffix_start = usize::min(
            hunk.lines().len(),
            suffix_start
                .unwrap_or_else(|| hunk.lines().len())
                .saturating_add(DISPLAYED_CONTEXT_LINES),
        );

        let context_prefix: Vec<Line> = (&hunk.lines()[0..diff_start]).into();
        let lines: Vec<Line> = (&hunk.lines()[diff_start..suffix_start]).into();
        let context_suffix: Vec<Line> = (&hunk.lines()[suffix_start..]).into();
        let end_line = &hunk.lines()[hunk.lines().len() - 1];

        Self {
            start_line: (
                hunk.lines()[0].source_line_no.unwrap_or_default(),
                hunk.lines()[0].target_line_no.unwrap_or_default(),
            ),
            context_prefix: if context_prefix.is_empty() {
                None
            } else {
                Some(context_prefix)
            },
            diff: lines,
            context_suffix: if context_suffix.is_empty() {
                None
            } else {
                Some(context_suffix)
            },
            end_line: (
                end_line.source_line_no.unwrap_or_default(),
                end_line.target_line_no.unwrap_or_default(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unidiff::PatchSet;

    #[test]
    fn test_hunk_to_section() {
        let diff_str = r#"
diff --git a/some/path/to/file.rs b/some/path/to/file.rs
index 422b64415..9561909ed 100644
--- a/some/path/to/file.rs
+++ b/some/path/to/file.rs
@@ -1,31 +1,31 @@
 00
 01
 02
 03
 04
 05
 06
 07
 08
 09
 10
 11
 12
 13
 14
-oops
+15
 16
 17
 18
 19
 20
 21
 22
 23
 24
 25
 26
 27
 28
 29
 30
        "#;
        let mut patch = PatchSet::new();
        patch.parse(diff_str).unwrap();

        let hunk = &patch.files()[0].hunks()[0];
        let section: Section = hunk.into();

        assert_eq!((1, 1), section.start_line);
        assert_eq!((31, 31), section.end_line);
        assert_eq!(5, section.context_prefix.as_ref().unwrap().len());
        assert_eq!(22, section.diff.len());
        assert_eq!(
            "05", section.diff[0].value,
            "the first line of the diff should be {DISPLAYED_CONTEXT_LINES} lines before the first modified line"
        );
        assert_eq!(
            "25", section.diff[21].value,
            "the last line of the diff should be {DISPLAYED_CONTEXT_LINES} lines after the last modified line"
        );
        assert_eq!(5, section.context_suffix.as_ref().unwrap().len());
        assert_eq!("26", section.context_suffix.as_ref().unwrap()[0].value);
        assert_eq!("30", section.context_suffix.as_ref().unwrap()[4].value);

        let diff_str = r#"
diff --git a/some/path/to/file.rs b/some/path/to/file.rs
index 422b64415..9561909ed 100644
--- a/some/path/to/file.rs
+++ b/some/path/to/file.rs
@@ -1,38 +1,36 @@
 00
 01
 02
 03
 04
 05
 06
 07
 08
 09
 10
 11
 12
 13
 14
-oops
+15
 16
 17
 18
 19
-oops1
-oops2
-oops3
+20
 21
 22
 23
 24
 25
 26
 27
 28
 29
 31
 32
 33
 34
 35
 36
        "#;
        let mut patch = PatchSet::new();
        patch.parse(diff_str).unwrap();

        let hunk = &patch.files()[0].hunks()[0];
        let section: Section = hunk.into();

        assert_eq!((1, 1), section.start_line);
        assert_eq!((38, 36), section.end_line);
        assert_eq!(5, section.context_prefix.as_ref().unwrap().len());
        assert_eq!(30, section.diff.len());
        assert_eq!(
            "05", section.diff[0].value,
            "the first line of the diff should be {DISPLAYED_CONTEXT_LINES} lines before the first modified line"
        );
        assert_eq!(
            "31", section.diff[29].value,
            "the last line of the diff should be {DISPLAYED_CONTEXT_LINES} lines after the last modified line"
        );
        assert_eq!(5, section.context_suffix.as_ref().unwrap().len());
        assert_eq!("32", section.context_suffix.as_ref().unwrap()[0].value);
        assert_eq!("36", section.context_suffix.as_ref().unwrap()[4].value);
    }
}
