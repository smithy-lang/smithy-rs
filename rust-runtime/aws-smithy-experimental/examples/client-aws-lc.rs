/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_experimental::hyper_1_0::{CryptoMode, HyperClientBuilder};
#[tokio::main]

async fn main() {
    let _client = HyperClientBuilder::new()
        .crypto_mode(CryptoMode::AwsLc)
        .build_https();

    // A FIPS client can also be created. Note that this has a more complex build environment required.
    let _client = HyperClientBuilder::new()
        .crypto_mode(CryptoMode::AwsLcFips)
        .build_https();
}
