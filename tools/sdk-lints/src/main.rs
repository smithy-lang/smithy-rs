use anyhow::{bail, Context, Result};
use clap::{App, Arg, SubCommand};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

fn main() -> Result<()> {
    let matches = clap_app().get_matches();
    if let Some(subcommand) = matches.subcommand_matches("check") {
        let all = subcommand.is_present("all");
        if subcommand.is_present("readme") || all {
            check_readmes()?;
        }
    } else if let Some(subcommand) = matches.subcommand_matches("fix") {
        let all = subcommand.is_present("all");
        if subcommand.is_present("readme") || all {
            fix_readmes()?;
        }
    } else {
        clap_app().print_long_help().unwrap();
    }
    Ok(())
}

fn clap_app() -> App<'static, 'static> {
    App::new("smithy-rs linter")
        .subcommand(
            SubCommand::with_name("check")
                .arg(
                    Arg::with_name("readme")
                        .takes_value(false)
                        .required(false)
                        .long("readme"),
                )
                .arg(
                    Arg::with_name("all")
                        .takes_value(false)
                        .required(false)
                        .long("all"),
                ),
        )
        .subcommand(
            SubCommand::with_name("fix")
                .arg(
                    Arg::with_name("readme")
                        .takes_value(false)
                        .required(false)
                        .long("readme"),
                )
                .arg(
                    Arg::with_name("all")
                        .takes_value(false)
                        .required(false)
                        .long("all"),
                ),
        )
}

fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .with_context(|| "couldn't load repo root")?;
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn ls(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(path.as_ref())
        .with_context(|| format!("failed to ls: {:?}", path.as_ref()))?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?)
}

fn smithy_rs_crates() -> Result<Vec<PathBuf>> {
    let smithy_crate_root = repo_root()?.join("rust-runtime");
    ls(smithy_crate_root)
}

fn aws_runtime_crates() -> Result<Vec<PathBuf>> {
    let aws_crate_root = repo_root()?.join("aws").join("rust-runtime");
    ls(aws_crate_root)
}

fn check_readmes() -> Result<()> {
    let smithy_crates = smithy_rs_crates().with_context(|| "couldn't load smithy root")?;
    let aws_crates = aws_runtime_crates().with_context(|| "couldn't load aws crate root")?;
    let no_readme = smithy_crates
        .into_iter()
        .chain(aws_crates.into_iter())
        .filter(|dir| dir.is_dir() && dir.join("Cargo.toml").exists())
        .filter(|dir| !dir.join("README.md").exists());

    let mut failed = 0;
    for bad_crate in no_readme {
        eprintln!(
            "{:?} is missing a README",
            bad_crate
                .strip_prefix(repo_root()?)
                .expect("must be relative to repo root")
        );
        failed += 1;
    }
    if failed > 0 {
        bail!("{} crates were missing READMEs", failed)
    } else {
        eprintln!("All crates have READMEs!");
        Ok(())
    }
}

fn fix_readmes() -> Result<()> {
    let smithy_crates = smithy_rs_crates().with_context(|| "couldn't load smithy root")?;
    let aws_crates = aws_runtime_crates().with_context(|| "couldn't load aws crate root")?;
    let readmes = smithy_crates
        .into_iter()
        .chain(aws_crates.into_iter())
        .map(|pkg| pkg.join("README.md"));
    let mut num_fixed = 0;
    for readme in readmes {
        num_fixed += fix_readme(readme)?.then(|| 1).unwrap_or_default();
    }
    if num_fixed > 0 {
        bail!("Updated {} READMEs with footer.", num_fixed);
    } else {
        eprintln!("All READMEs have correct footers");
        Ok(())
    }
}

fn anchors(name: &str) -> (String, String) {
    (
        format!("{}{} -->", ANCHOR_START, name),
        format!("{}{} -->", ANCHOR_END, name),
    )
}

const ANCHOR_START: &str = "<!-- anchor_start:";
const ANCHOR_END: &str = "<!-- anchor_end:";

const README_FOOTER: &str = "This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) \
and the [smithy-rs](https://github.com/awslabs/smithy-rs) code generator. In most cases, it should not be used directly.";

fn fix_readme(path: impl AsRef<Path>) -> Result<bool> {
    let mut contents =
        fs::read_to_string(path.as_ref()).with_context(|| "failure to read readme")?;
    let updated = replace_anchor(&mut contents, &anchors("footer"), README_FOOTER)?;
    fs::write(path.as_ref(), contents)?;
    Ok(updated)
}

fn replace_anchor(
    haystack: &mut String,
    anchors: &(String, String),
    new_content: &str,
) -> Result<bool> {
    let anchor_start = anchors.0.as_str();
    let anchor_end = anchors.1.as_str();
    let start = haystack.find(&anchor_start);
    if start.is_none() {
        if haystack.contains(anchor_end) {
            bail!("found end anchor but no start anchor");
        }
        haystack.push('\n');
        haystack.push_str(anchor_start);
        haystack.push_str(new_content);
        haystack.push_str(anchor_end);
        return Ok(true);
    }
    let start = start.unwrap_or_else(|| haystack.find(&anchor_start).expect("must be present"));
    let end = match haystack[start..].find(&anchor_end) {
        Some(end) => end + start,
        None => bail!("expected matching end anchor {}", anchor_end),
    };
    let prefix = &haystack[..start + anchor_start.len()];
    let suffix = &haystack[end..];
    let mut out = String::new();
    out.push_str(prefix);
    out.push_str(new_content);
    out.push_str(suffix);
    if haystack != &out {
        *haystack = out;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod test {
    use crate::{anchors, replace_anchor};

    #[test]
    fn updates_empty() {
        let mut text = "this is the start".to_string();
        assert!(replace_anchor(&mut text, &anchors("foo"), "hello!").unwrap());
        assert_eq!(
            text,
            "this is the start\n<!-- anchor_start:foo -->hello!<!-- anchor_end:foo -->"
        );
    }

    #[test]
    fn updates_existing() {
        let mut text =
            "this is the start\n<!-- anchor_start:foo -->hello!<!-- anchor_end:foo -->".to_string();
        assert!(replace_anchor(&mut text, &anchors("foo"), "goodbye!").unwrap());
        assert_eq!(
            text,
            "this is the start\n<!-- anchor_start:foo -->goodbye!<!-- anchor_end:foo -->"
        );

        // no replacement should return false
        assert_eq!(
            replace_anchor(&mut text, &anchors("foo"), "goodbye!").unwrap(),
            false
        )
    }
}

// TODO:
// fn check_authors()
// fn check_docs_all_features()
// fn check_doc_targets()
// fn check_license()
