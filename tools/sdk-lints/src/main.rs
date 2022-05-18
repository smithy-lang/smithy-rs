/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::changelog::ChangelogNext;
use crate::copyright::CopyrightHeader;
use crate::lint::{Check, Fix, Lint, LintError, Mode};
use crate::lint_cargo_toml::{CrateAuthor, CrateLicense, DocsRs};
use crate::readmes::{ReadmesExist, ReadmesHaveFooters};
use crate::todos::TodosHaveContext;
use anyhow::{bail, Context, Result};
use changelog::ReleaseMetadata;
use clap::Parser;
use lazy_static::lazy_static;
use ordinal::Ordinal;
use std::env::set_current_dir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};
use time::OffsetDateTime;

mod anchor;
mod changelog;
mod copyright;
mod lint;
mod lint_cargo_toml;
mod readmes;
mod todos;

fn load_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .with_context(|| "couldn't load repo root")?;
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

#[derive(Debug, Parser)]
enum Args {
    Check {
        #[clap(long)]
        all: bool,
        #[clap(long)]
        readme: bool,
        #[clap(long)]
        cargo_toml: bool,
        #[clap(long)]
        docsrs_metadata: bool,
        #[clap(long)]
        changelog: bool,
        #[clap(long)]
        license: bool,
        #[clap(long)]
        todos: bool,
    },
    Fix {
        #[clap(long)]
        readme: bool,
        #[clap(long)]
        docsrs_metadata: bool,
        #[clap(long)]
        all: bool,
        #[clap(long)]
        dry_run: Option<bool>,
    },
    UpdateChangelog {
        /// Whether or not independent crate versions are being used (defaults to false)
        #[clap(long)]
        independent_versioning: bool,
        /// Optional path to output a release manifest file to
        #[clap(long)]
        release_manifest_output_path: Option<PathBuf>,
    },
}

fn load_vcs_files() -> Result<Vec<PathBuf>> {
    let tracked_files = Command::new("git")
        .arg("ls-tree")
        .arg("-r")
        .arg("HEAD")
        .arg("--name-only")
        .current_dir(load_repo_root()?)
        .output()
        .context("couldn't load VCS tracked files")?;
    let mut output = String::from_utf8(tracked_files.stdout)?;
    let changed_files = Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .output()?;
    output.push_str(std::str::from_utf8(changed_files.stdout.as_slice())?);
    let files = output
        .lines()
        .map(|line| PathBuf::from(line.trim().to_string()));
    Ok(files.collect())
}

lazy_static! {
    static ref REPO_ROOT: PathBuf = load_repo_root().unwrap();
    static ref VCS_FILES: Vec<PathBuf> = load_vcs_files().unwrap();
}

fn repo_root() -> &'static Path {
    REPO_ROOT.as_path()
}

fn ok<T>(errors: Vec<T>) -> anyhow::Result<()> {
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("Lint errors occurred");
    }
}

fn main() -> Result<()> {
    set_current_dir(repo_root())?;
    let opt = Args::parse();
    match opt {
        Args::Check {
            all,
            readme,
            cargo_toml,
            docsrs_metadata,
            changelog,
            license,
            todos,
        } => {
            let mut errs = vec![];
            if readme || all {
                errs.extend(ReadmesExist.check_all()?);
                errs.extend(ReadmesHaveFooters.check_all()?);
            }
            if cargo_toml || all {
                errs.extend(CrateAuthor.check_all()?);
                errs.extend(CrateLicense.check_all()?);
            }

            if docsrs_metadata || all {
                errs.extend(DocsRs.check_all()?);
            }

            if license || all {
                errs.extend(CopyrightHeader.check_all()?);
            }
            if changelog || all {
                errs.extend(ChangelogNext.check_all()?);
            }
            if todos || all {
                errs.extend(TodosHaveContext.check_all()?);
            }
            ok(errs)?
        }
        Args::Fix {
            readme,
            docsrs_metadata,
            all,
            dry_run,
        } => {
            let dry_run = match dry_run.unwrap_or(false) {
                true => Mode::DryRun,
                false => Mode::NoDryRun,
            };
            if readme || all {
                ok(ReadmesHaveFooters.fix_all(dry_run)?)?;
            }
            if docsrs_metadata || all {
                ok(DocsRs.fix_all(dry_run)?)?;
            }
        }
        Args::UpdateChangelog {
            independent_versioning,
            release_manifest_output_path,
        } => {
            let now = OffsetDateTime::now_local()?;
            let changelog_next_path = repo_root().join("CHANGELOG.next.toml");
            let changelog_path = repo_root().join("CHANGELOG.md");
            let aws_changelog_path = repo_root().join("aws/SDK_CHANGELOG.md");
            if independent_versioning {
                let smithy_rs_metadata =
                    date_based_release_metadata(now, "smithy-rs-release-manifest.json");
                let sdk_metadata =
                    date_based_release_metadata(now, "aws-sdk-rust-release-manifest.json");
                changelog::update_changelogs(
                    changelog_next_path,
                    changelog_path,
                    aws_changelog_path,
                    &smithy_rs_metadata,
                    &sdk_metadata,
                    release_manifest_output_path.as_deref(),
                )?
            } else {
                let auto = auto_changelog_meta()?;
                let smithy_rs_metadata = version_based_release_metadata(
                    now,
                    &auto.smithy_version,
                    "smithy-rs-release-manifest.json",
                );
                let sdk_metadata = version_based_release_metadata(
                    now,
                    &auto.sdk_version,
                    "aws-sdk-rust-release-manifest.json",
                );
                changelog::update_changelogs(
                    changelog_next_path,
                    changelog_path,
                    aws_changelog_path,
                    &smithy_rs_metadata,
                    &sdk_metadata,
                    release_manifest_output_path.as_deref(),
                )?
            }
        }
    }
    Ok(())
}

struct ChangelogMeta {
    smithy_version: String,
    sdk_version: String,
}

fn date_based_release_metadata(
    now: OffsetDateTime,
    manifest_name: impl Into<String>,
) -> ReleaseMetadata {
    ReleaseMetadata {
        title: date_title(&now),
        tag: format!(
            "release-{year}-{month:02}-{day:02}",
            year = now.date().year(),
            month = u8::from(now.date().month()),
            day = now.date().day()
        ),
        manifest_name: manifest_name.into(),
    }
}

fn version_based_release_metadata(
    now: OffsetDateTime,
    version: &str,
    manifest_name: impl Into<String>,
) -> ReleaseMetadata {
    ReleaseMetadata {
        title: format!(
            "v{version} ({date})",
            version = version,
            date = date_title(&now)
        ),
        tag: format!("v{version}", version = version),
        manifest_name: manifest_name.into(),
    }
}

fn date_title(now: &OffsetDateTime) -> String {
    format!(
        "{month} {day}, {year}",
        month = now.date().month(),
        day = Ordinal(now.date().day()),
        year = now.date().year()
    )
}

/// Discover the new version for the changelog from gradle.properties and the date.
fn auto_changelog_meta() -> Result<ChangelogMeta> {
    let gradle_props = fs::read_to_string(repo_root().join("gradle.properties"))?;
    let load_gradle_prop = |key: &str| {
        let prop = gradle_props
            .lines()
            .flat_map(|line| line.strip_prefix(key))
            .flat_map(|prop| prop.strip_prefix('='))
            .next();
        prop.map(|prop| prop.to_string())
            .ok_or_else(|| anyhow::Error::msg(format!("missing expected gradle property: {key}")))
    };
    let smithy_version = load_gradle_prop("smithy.rs.runtime.crate.version")?;
    let sdk_version = load_gradle_prop("aws.sdk.version")?;
    Ok(ChangelogMeta {
        smithy_version,
        sdk_version,
    })
}

fn ls(path: impl AsRef<Path>) -> Result<impl Iterator<Item = PathBuf>> {
    Ok(fs::read_dir(path.as_ref())
        .with_context(|| format!("failed to ls: {:?}", path.as_ref()))?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?
        .into_iter())
}

fn smithy_rs_crates() -> Result<impl Iterator<Item = PathBuf>> {
    let smithy_crate_root = repo_root().join("rust-runtime");
    Ok(ls(smithy_crate_root)?.filter(|path| is_crate(path.as_path())))
}

fn is_crate(path: &Path) -> bool {
    path.is_dir() && path.join("Cargo.toml").exists()
}

fn aws_runtime_crates() -> Result<impl Iterator<Item = PathBuf>> {
    let aws_crate_root = repo_root().join("aws").join("rust-runtime");
    Ok(ls(aws_crate_root)?.filter(|path| is_crate(path.as_path())))
}

fn all_runtime_crates() -> Result<impl Iterator<Item = PathBuf>> {
    Ok(aws_runtime_crates()?.chain(smithy_rs_crates()?))
}

fn all_cargo_tomls() -> Result<impl Iterator<Item = PathBuf>> {
    Ok(all_runtime_crates()?.map(|pkg| pkg.join("Cargo.toml")))
}

#[cfg(test)]
mod tests {
    use crate::{date_based_release_metadata, version_based_release_metadata};
    use time::OffsetDateTime;

    #[test]
    fn test_date_based_release_metadata() {
        let now = OffsetDateTime::from_unix_timestamp(100_000_000).unwrap();
        let result = date_based_release_metadata(now, "some-manifest.json");
        assert_eq!("March 3rd, 1973", result.title);
        assert_eq!("release-1973-03-03", result.tag);
        assert_eq!("some-manifest.json", result.manifest_name);
    }

    #[test]
    fn test_version_based_release_metadata() {
        let now = OffsetDateTime::from_unix_timestamp(100_000_000).unwrap();
        let result = version_based_release_metadata(now, "0.11.0", "some-other-manifest.json");
        assert_eq!("v0.11.0 (March 3rd, 1973)", result.title);
        assert_eq!("v0.11.0", result.tag);
        assert_eq!("some-other-manifest.json", result.manifest_name);
    }
}
