/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[pyo3_async_runtimes::tokio::main]
async fn main() -> pyo3::PyResult<()> {
    pyo3_async_runtimes::testing::main().await
}

mod bytestream;
