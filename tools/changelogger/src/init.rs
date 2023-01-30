/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::render::EXAMPLE_ENTRY;
use clap::Parser;
use std::io::Write;

#[derive(Parser, Debug, Eq, PartialEq)]
pub struct InitArgs {}

pub fn subcommand_init(_args: &InitArgs) -> anyhow::Result<()> {
    writeln!(std::io::stdout(), "{}", EXAMPLE_ENTRY)?;
    Ok(())
}
