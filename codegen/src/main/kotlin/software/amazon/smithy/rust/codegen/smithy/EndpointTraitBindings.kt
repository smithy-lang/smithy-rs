/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.generators.rustFormatString
import software.amazon.smithy.rust.codegen.util.inputShape

fun EndpointTrait.prefixFormatString(): String {
    return this.hostPrefix.rustFormatString("", "")
}

fun RuntimeConfig.smithyHttp() = CargoDependency.SmithyHttp(this).asType()

class EndpointTraitBindings(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    operationShape: OperationShape,
    private val endpointTrait: EndpointTrait
) {
    private val inputShape = operationShape.inputShape(model)
    private val smithyHttp = symbolProvider.config().runtimeConfig.smithyHttp()

    /**
     * Render the `EndpointPrefix` struct. [input] refers to the symbol referring to the input of this operation.
     */
    fun render(writer: RustWriter, input: String) {
        if (endpointTrait.hostPrefix.labels.isEmpty()) {
            // if there are no labels, we don't need string formatting
            writer.rust("#T::endpoint::EndpointPrefix::new(${endpointTrait.prefixFormatString()})", smithyHttp)
        } else {
            writer.withBlock(
                "#T::endpoint::EndpointPrefix::new(format!(${endpointTrait.prefixFormatString()}, ",
                "))",
                smithyHttp
            ) {
                endpointTrait.hostPrefix.labels.forEach { label ->
                    val member = inputShape.getMember(label.content).get()
                    rust("${label.content} = $input.${symbolProvider.toMemberName(member)}, ")
                }
            }
        }
    }
}
