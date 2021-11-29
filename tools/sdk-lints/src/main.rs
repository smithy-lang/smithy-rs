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
    } else {
        clap_app().print_long_help().unwrap();
    }
    Ok(())
}

fn clap_app() -> App<'static, 'static> {
    App::new("smithy-rs linter").subcommand(
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

// TODO:
// fn check_authors()
// fn check_docs_all_features()
// fn check_doc_targets()
// fn check_license()
