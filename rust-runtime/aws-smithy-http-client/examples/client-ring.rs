/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_client::hyper_1::{CryptoMode, HyperClientBuilder};

fn main() {
    let _client = HyperClientBuilder::new()
        .crypto_mode(CryptoMode::Ring)
        .build_https();
}
