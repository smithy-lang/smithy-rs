/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.generators.client

import software.amazon.smithy.rust.codegen.core.customize.AdHocSection

sealed class CustomizableOperationSection(name: String) : AdHocSection(name) {
    /** Write custom code into a customizable operation's impl block */
    object CustomizableOperationImpl : CustomizableOperationSection("CustomizableOperationImpl")
}
