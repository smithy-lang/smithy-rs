/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

type BoxError = Box<dyn std::error::Error>;

/// please-fmt
#[derive(argh::FromArgs)]
struct Args {
    /// path to recursively format (defaults to current directory)
    #[argh(option)]
    path: Option<String>,
}

impl Args {
    fn path(&self) -> Result<Utf8PathBuf, BoxError> {
        Ok(if let Some(path) = self.path.as_ref() {
            path.into()
        } else {
            std::env::current_dir()?.canonicalize()?.try_into()?
        })
    }
}

fn main() -> Result<(), BoxError> {
    let args: Args = argh::from_env();
    let path = args.path()?;
    format_path(&path)?;

    Ok(())
}

fn format_path(path: &Utf8Path) -> Result<(), BoxError> {
    let mut errors = Vec::new();
    for entry in path.read_dir_utf8()? {
        let entry = entry?;
        let extension = entry.path().extension();
        let file_type = entry.file_type()?;
        if extension == Some("rs") && (file_type.is_file() || file_type.is_symlink()) {
            if let Err(err) = format_file(entry.path()) {
                errors.push((entry.path().to_string(), err));
            }
        } else if file_type.is_dir() || file_type.is_symlink() {
            format_path(entry.path())?;
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors
            .into_iter()
            .map(|entry| format!("error: {}: {}", entry.0, entry.1))
            .collect::<Vec<String>>()
            .join("\n")
            .into())
    }
}

fn format_file(path: &Utf8Path) -> Result<(), BoxError> {
    let contents =
        fs::read_to_string(path).map_err(|err| format!("failed to read file {path}: {err}"))?;
    let ast =
        syn::parse_file(&contents).map_err(|err| format!("failed to parse file {path}: {err}"))?;
    let formatted = prettyplease::unparse(&ast);
    fs::write(path, formatted).map_err(|err| format!("failed to write file {path}: {err}"))?;
    Ok(())
}
