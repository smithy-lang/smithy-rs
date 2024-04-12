/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::page::{File, Page, PageTracker};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::process;
use unidiff::PatchSet;

mod html;
mod page;

#[derive(Debug, Parser)]
#[clap(name = "difftags", version)]
#[clap(about = "Diff to HTML conversion tool")]
struct Cli {
    /// Directory to output to
    #[clap(short, long)]
    output_dir: PathBuf,

    /// Diff file to convert to HTML, in unified diff format
    input: PathBuf,

    /// Maximum files per page of HTML
    #[clap(long, default_value = "15")]
    max_files_per_page: usize,

    /// Maximum modified lines per page of HTML
    #[clap(long, default_value = "1000")]
    max_lines_per_page: usize,

    /// Title to apply to the diff
    #[clap(long)]
    title: Option<String>,

    /// Optional subtitle to appear under the title
    #[clap(long)]
    subtitle: Option<String>,
}

fn main() {
    let args = Cli::parse();
    let diff_str = match fs::read_to_string(args.input) {
        Ok(diff_str) => diff_str,
        Err(err) => {
            eprintln!("failed to load the input diff file: {err}");
            process::exit(1)
        }
    };

    let mut patch = PatchSet::new();
    if let Err(err) = patch.parse(&diff_str) {
        eprintln!("failed to parse the input diff file: {err}");
        process::exit(1)
    }

    let mut pages = Vec::new();
    let mut page_tracker = PageTracker::new(args.max_files_per_page, args.max_lines_per_page);
    let mut current_page = Page::default();
    for patched_file in patch {
        if page_tracker.next_file_is_page_boundary() {
            pages.push(current_page);
            current_page = Page::default();
            page_tracker.reset();
        }
        let file: File = patched_file.into();
        page_tracker.total_modified_lines(
            file.sections()
                .iter()
                .map(|section| {
                    section
                        .diff
                        .iter()
                        .filter(|line| line.line_type != " ")
                        .count()
                })
                .sum(),
        );
        current_page.files.push(file);
    }
    pages.push(current_page);

    if let Err(err) = html::write_html(&args.output_dir, args.title, args.subtitle, &pages) {
        eprintln!("failed to write HTML: {err}");
        process::exit(1)
    }
}
