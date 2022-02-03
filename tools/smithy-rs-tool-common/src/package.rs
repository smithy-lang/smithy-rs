/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub const SMITHY_PREFIX: &str = "aws-smithy-";
pub const SDK_PREFIX: &str = "aws-sdk-";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PackageCategory {
    SmithyRuntime,
    AwsRuntime,
    AwsSdk,
    Unknown,
}

impl PackageCategory {
    /// Returns true if the category is `AwsRuntime` or `AwsSdk`
    pub fn is_sdk(&self) -> bool {
        matches!(self, PackageCategory::AwsRuntime | PackageCategory::AwsSdk)
    }

    /// Categorizes a package based on its name
    pub fn from_package_name(name: &str) -> PackageCategory {
        if name.starts_with(SMITHY_PREFIX) {
            PackageCategory::SmithyRuntime
        } else if name.starts_with(SDK_PREFIX) {
            PackageCategory::AwsSdk
        } else if name.starts_with("aws-") {
            PackageCategory::AwsRuntime
        } else {
            PackageCategory::Unknown
        }
    }
}
