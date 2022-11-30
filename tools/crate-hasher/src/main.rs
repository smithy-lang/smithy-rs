/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Discovers files relevant to the build of a given crate, and
//! prints out a determistic SHA-256 of the entire crate contents.

use anyhow::Result;
use clap::Parser;
use file_list::FileList;
use std::path::PathBuf;

mod file_list;

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    location: PathBuf,
}

pub fn main() -> Result<()> {
    let file_list = FileList::discover(&Args::parse().location)?;
    println!("{}", file_list.sha256());
    Ok(())
}
