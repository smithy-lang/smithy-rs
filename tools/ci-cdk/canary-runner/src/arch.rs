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

impl From<Arch> for aws_sdk_lambda::types::Architecture {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::X86_64 => aws_sdk_lambda::types::Architecture::X8664,
            Arch::Aarch64 => aws_sdk_lambda::types::Architecture::Arm64,
        }
    }
}

impl From<Arch> for aws_sdk_lambda::types::Runtime {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::X86_64 => aws_sdk_lambda::types::Runtime::Providedal2,
            Arch::Aarch64 => aws_sdk_lambda::types::Runtime::Providedal2023,
        }
    }
}
