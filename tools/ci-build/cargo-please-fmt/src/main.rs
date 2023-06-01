/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

type BoxError = Box<dyn std::error::Error>;

/// cargo-please-fmt
#[derive(argh::FromArgs)]
struct Args {
    #[argh(positional)]
    _cargo_subcommand: String,

    /// path to Cargo.toml
    #[argh(option)]
    manifest_path: Option<String>,
}

impl Args {
    fn manifest_path(&self) -> Result<Utf8PathBuf, BoxError> {
        Ok(if let Some(manifest_path) = self.manifest_path.as_ref() {
            manifest_path.into()
        } else {
            std::env::current_dir()?
                .canonicalize()?
                .join("Cargo.toml")
                .try_into()?
        })
    }
}

fn main() -> Result<(), BoxError> {
    let args: Args = argh::from_env();
    let manifest_path = args.manifest_path()?;

    for package_root in discover_package_roots(&manifest_path)? {
        format_path(&package_root)?;
    }

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

fn discover_package_roots(manifest_path: &Utf8Path) -> Result<Vec<Utf8PathBuf>, BoxError> {
    let mut package_roots_to_format = Vec::new();
    let cargo_metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()?;
    for member in cargo_metadata.workspace_members {
        if let Some(package) = cargo_metadata.packages.iter().find(|p| p.id == member) {
            let package_path: Utf8PathBuf = package
                .manifest_path
                .canonicalize()?
                .parent()
                .expect("parent path")
                .to_path_buf()
                .try_into()?;
            package_roots_to_format.push(package_path);
        } else {
            return Err(format!("failed to find workspace package: {member}").into());
        }
    }
    Ok(package_roots_to_format)
}
