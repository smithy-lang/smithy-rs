/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[pyo3_asyncio::tokio::main]
async fn main() -> pyo3::PyResult<()> {
    pyo3_asyncio::testing::main().await
}

mod bytestream;
