/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    webassembly::test().await
}
