/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl std::str::FromStr for Arch {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "x86_64" => Ok(Arch::X86_64),
            "aarch64" => Ok(Arch::Aarch64),
            _ => bail!("Invalid architecture: {}. Must be x86_64 or aarch64", s),
        }
    }
}
