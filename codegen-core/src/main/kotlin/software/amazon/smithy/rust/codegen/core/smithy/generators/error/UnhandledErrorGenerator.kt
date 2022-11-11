/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

internal fun unhandledError(): RuntimeType = RuntimeType.forInlineFun("Unhandled", RustModule.Error) {
    docs(
        """
        An unexpected error occurred (e.g., invalid JSON returned by the service or an unknown error code)

        Call [`Error::source`](std::error::Error::source) for more details about the underlying cause.
        """,
    )
    rust("##[derive(Debug)]")
    rustBlock("pub struct Unhandled") {
        rust("source: Box<dyn #T + Send + Sync + 'static>", RuntimeType.StdError)
    }
    rustBlock("impl Unhandled") {
        rustBlock("pub(crate) fn new(source: Box<dyn #T + Send + Sync + 'static>) -> Self", RuntimeType.StdError) {
            rust("Self { source }")
        }
    }
    rustBlock("impl std::fmt::Display for Unhandled") {
        rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error>") {
            rust("write!(f, \"unhandled error\")")
        }
    }
    rustBlock("impl std::error::Error for Unhandled") {
        rustBlock("fn source(&self) -> Option<&(dyn std::error::Error + 'static)>") {
            rust("Some(self.source.as_ref() as _)")
        }
    }
}
