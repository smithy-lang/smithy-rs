/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::lint::LintError;
use crate::{Check, Lint, VCS_FILES};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

// All "TODOs" must include (...) that gives them context
pub(crate) struct TodosHaveContext;

const IGNORE_DIRS: &[&str] = &["tools/ci-build/sdk-lints/src/todos.rs"];

impl Lint for TodosHaveContext {
    fn name(&self) -> &str {
        "TODOs include context"
    }

    fn files_to_check(&self) -> anyhow::Result<Vec<PathBuf>> {
        fn validate_todos(extension: Option<&OsStr>) -> bool {
            extension
                .map(|ext| {
                    ext.eq_ignore_ascii_case("rs")
                        || ext.eq_ignore_ascii_case("toml")
                        || ext.eq_ignore_ascii_case("txt")
                        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2077)
                        // || ext.eq_ignore_ascii_case("md")
                        || ext.eq_ignore_ascii_case("sh")
                        || ext.eq_ignore_ascii_case("py")
                        || ext.eq_ignore_ascii_case("smithy")
                        || ext.eq_ignore_ascii_case("kt")
                        || ext.eq_ignore_ascii_case("kts")
                })
                .unwrap_or(false)
        }
        Ok(VCS_FILES
            .iter()
            .filter(|path| !IGNORE_DIRS.iter().any(|dir| path.starts_with(dir)))
            .filter(|f| validate_todos(f.extension()))
            .cloned()
            .collect())
    }
}

impl Check for TodosHaveContext {
    fn check(&self, path: impl AsRef<Path>) -> anyhow::Result<Vec<LintError>> {
        let contents = match fs::read_to_string(path.as_ref()) {
            Ok(contents) => contents,
            Err(err) if format!("{err}").contains("No such file or directory") => {
                eprintln!("Note: {} does not exist", path.as_ref().display());
                return Ok(vec![]);
            }
            e @ Err(_) => e?,
        };
        let mut errs = vec![];
        for todo in contents.split("TODO").skip(1) {
            if !todo.starts_with('(') {
                let todo_line = todo.lines().next().unwrap_or_default();
                errs.push(LintError::new(format!(
                    "TODO without context: `TODO{todo_line}`"
                )))
            }
        }
        Ok(errs)
    }
}
