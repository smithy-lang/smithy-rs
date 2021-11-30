use anyhow::{bail, Context, Result};
use clap::{App, Arg, SubCommand};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

mod anchor;

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

fn ls(path: impl AsRef<Path>) -> Result<impl Iterator<Item = PathBuf>> {
    Ok(fs::read_dir(path.as_ref())
        .with_context(|| format!("failed to ls: {:?}", path.as_ref()))?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?
        .into_iter())
}

fn smithy_rs_crates() -> Result<impl Iterator<Item = PathBuf>> {
    let smithy_crate_root = repo_root()?.join("rust-runtime");
    Ok(ls(smithy_crate_root)?.filter(|path| is_crate(path.as_path())))
}

fn is_crate(path: &Path) -> bool {
    path.is_dir() && path.join("Cargo.toml").exists()
}

fn aws_runtime_crates() -> Result<impl Iterator<Item = PathBuf>> {
    let aws_crate_root = repo_root()?.join("aws").join("rust-runtime");
    Ok(ls(aws_crate_root)?.filter(|path| is_crate(path.as_path())))
}

fn all_runtime_crates() -> Result<impl Iterator<Item = PathBuf>> {
    Ok(aws_runtime_crates()?.chain(smithy_rs_crates()?))
}

fn check_readmes() -> Result<()> {
    let no_readme = all_runtime_crates()?.filter(|dir| !dir.join("README.md").exists());

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
    let readmes = all_runtime_crates()?.map(|pkg| pkg.join("README.md"));
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

const README_FOOTER: &str = "\nThis crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) \
and the [smithy-rs](https://github.com/awslabs/smithy-rs) code generator. In most cases, it should not be used directly.\n";

fn fix_readme(path: impl AsRef<Path>) -> Result<bool> {
    let mut contents = fs::read_to_string(path.as_ref())
        .with_context(|| format!("failure to read readme: {:?}", path.as_ref()))?;
    let updated = anchor::replace_anchor(&mut contents, &anchor::anchors("footer"), README_FOOTER)?;
    fs::write(path.as_ref(), contents)?;
    Ok(updated)
}

// TODO:
// fn check_authors()
// fn check_docs_all_features()
// fn check_doc_targets()
// fn check_license()
