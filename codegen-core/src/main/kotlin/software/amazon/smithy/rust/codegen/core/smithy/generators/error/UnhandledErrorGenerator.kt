/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

internal fun unhandledError(): RuntimeType = RuntimeType.forInlineFun("Unhandled", RustModule.Error) {
    docs(
        """
        An unexpected error occurred (e.g., invalid JSON returned by the service or an unknown error code)

        Call [`Error::source`](std::error::Error::source) for more details about the underlying cause.
        """,
    )
    rustTemplate(
        """
        ##[derive(Debug)]
        pub struct Unhandled {
            source: Box<dyn #{StdError} + Send + Sync + 'static>,
        }
        impl Unhandled {
            pub(crate) fn new(source: Box<dyn #{StdError} + Send + Sync + 'static>) -> Self {
                Self { source }
            }
        }
        impl std::fmt::Display for Unhandled {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                write!(f, "unhandled error")
            }
        }
        impl #{StdError} for Unhandled {
            fn source(&self) -> Option<&(dyn #{StdError} + 'static)> {
                Some(self.source.as_ref() as _)
            }
        }
        """,
        "StdError" to RuntimeType.StdError,
    )
}
