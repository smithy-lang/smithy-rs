/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.rust.codegen.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.rustlang.Visibility

object InlineSmithyDependency {
    fun forRustFile(file: String, visibility: Visibility = Visibility.PRIVATE, vararg additionalDependency: RustDependency): InlineDependency =
        InlineDependency.forRustFile(file, "aws-smithy-inlineable", visibility, *additionalDependency)
}
