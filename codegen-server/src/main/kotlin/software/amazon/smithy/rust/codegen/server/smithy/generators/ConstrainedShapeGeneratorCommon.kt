/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

/**
 * Functions shared amongst the constrained shape generators, to keep them DRY and consistent.
 */

fun rustDocsConstrainedTypeEpilogue(typeName: String) = """
    This is a constrained type because its corresponding modeled Smithy shape has one or more
    [constraint traits]. Use [`$typeName::try_from`] to construct values of this type.

    [constraint traits]: https://awslabs.github.io/smithy/1.0/spec/core/constraint-traits.html
"""

fun rustDocsTryFromMethod(typeName: String, inner: String) =
    "Constructs a `$typeName` from an [`$inner`], failing when the provided value does not satisfy the modeled constraints."

fun rustDocsInnerMethod(inner: String) =
    "Returns an immutable reference to the underlying [`$inner`]."

fun rustDocsIntoInnerMethod(inner: String) =
    "Consumes the value, returning the underlying [`$inner`]."
