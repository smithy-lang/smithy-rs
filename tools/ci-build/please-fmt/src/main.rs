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
    for entry in path.read_dir_utf8()? {
        let entry = entry?;
        let extension = entry.path().extension();
        let file_type = entry.file_type()?;
        if extension == Some("rs") && (file_type.is_file() || file_type.is_symlink()) {
            let contents = fs::read_to_string(entry.path())
                .map_err(|err| format!("failed to read file {}: {err}", entry.path()))?;
            let ast = syn::parse_file(&contents)
                .map_err(|err| format!("failed to parse file {}: {err}", entry.path()))?;
            let formatted = prettyplease::unparse(&ast);
            fs::write(entry.path(), formatted)
                .map_err(|err| format!("failed to write file {}: {err}", entry.path()))?;
        } else if file_type.is_dir() || file_type.is_symlink() {
            format_path(entry.path())?;
        }
    }
    Ok(())
}
