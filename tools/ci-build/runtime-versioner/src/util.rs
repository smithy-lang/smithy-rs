/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use std::path::Path;

pub fn utf8_path_buf(path: impl AsRef<Path>) -> Utf8PathBuf {
    let path: &Path = path.as_ref();
    <&Utf8Path>::try_from(path)
        .with_context(|| format!("gross path_buf: {path:?}"))
        .unwrap()
        .into()
}
