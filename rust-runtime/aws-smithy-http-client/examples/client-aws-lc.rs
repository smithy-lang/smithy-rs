/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_client::{Builder, CryptoMode};

#[tokio::main]
async fn main() {
    // feature = crypto-aws-lc
    let _client = Builder::new().crypto_mode(CryptoMode::AwsLc).build_https();

    // feature = crypto-aws-lc-fips
    // A FIPS client can also be created. Note that this has a more complex build environment required.
    let _client = Builder::new()
        .crypto_mode(CryptoMode::AwsLcFips)
        .build_https();
}
