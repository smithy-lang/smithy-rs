/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section

sealed class CustomizableOperationSection(name: String) : Section(name) {
    /** Write custom code into a customizable operation's impl block */
    data class CustomizableOperationImpl(
        val isRuntimeModeOrchestrator: Boolean,
    ) : CustomizableOperationSection("CustomizableOperationImpl")
}

abstract class CustomizableOperationCustomization : NamedCustomization<CustomizableOperationSection>()
