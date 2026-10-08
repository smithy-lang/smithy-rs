/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The embedded Smithy 2.0 prelude.

use crate::ast::Document;
use crate::loader::ModelLoader;
use std::sync::OnceLock;

/// The Smithy Java version whose prelude is embedded.
pub(crate) const PRELUDE_SOURCE_VERSION: &str = "1.74.0";

const PRELUDE_JSON: &str = include_str!("prelude/prelude-1.74.0.json");

/// The parsed prelude, shared read-only by every model in the process.
pub(crate) fn prelude() -> &'static Document {
    static PRELUDE: OnceLock<Document> = OnceLock::new();
    PRELUDE.get_or_init(|| {
        let source = format!("smithy.api prelude ({PRELUDE_SOURCE_VERSION})");
        ModelLoader::new()
            .disable_prelude()
            .parse_str(&source, PRELUDE_JSON)
            .expect("the embedded prelude is a valid Smithy 2.0 JSON AST document")
    })
}
