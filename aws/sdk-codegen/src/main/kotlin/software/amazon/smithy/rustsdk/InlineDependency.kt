/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.InlineDependency

object InlineAwsDependency {
    fun forRustFile(file: String): InlineDependency = InlineDependency.Companion.forRustFile(file, "aws-inlineable")
}
