/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section

/** Error customization sections */
sealed class ErrorSection(name: String) : Section(name) {
    /** Use this section to add additional trait implementations to the generated operation errors */
    data class OperationErrorAdditionalTraitImpls(val errorSymbol: Symbol, val allErrors: List<StructureShape>) :
        ErrorSection("OperationErrorAdditionalTraitImpls")

    /** Use this section to add additional trait implementations to the generated service error */
    class ServiceErrorAdditionalTraitImpls(val allErrors: List<StructureShape>) :
        ErrorSection("ServiceErrorAdditionalTraitImpls")
}

/** Customizations for generated errors */
abstract class ErrorCustomization : NamedCustomization<ErrorSection>()
