/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.http.rustFormatString
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.util.inputShape

fun EndpointTrait.prefixFormatString(): String {
    return this.hostPrefix.rustFormatString("", "")
}

fun RuntimeConfig.smithyHttp() = CargoDependency.SmithyHttp(this).asType()

class EndpointTraitBindings(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    operationShape: OperationShape,
    private val endpointTrait: EndpointTrait
) {
    private val inputShape = operationShape.inputShape(model)
    private val smithyHttp = runtimeConfig.smithyHttp()
    private val endpointPrefix = smithyHttp.member("endpoint::EndpointPrefix")

    /**
     * Render the `EndpointPrefix` struct. [input] refers to the symbol referring to the input of this operation.
     *
     * Generates code like:
     * ```rust
     * EndpointPrefix::new(format!("{}.aws.com", input.bucket));
     * ```
     *
     * The returned expression is a `Result<EndpointPrefix, UriError>`
     */
    fun render(writer: RustWriter, input: String) {
        // the Rust format pattern to make the endpoint prefix eg. "{}.foo"
        val formatLiteral = endpointTrait.prefixFormatString()
        if (endpointTrait.hostPrefix.labels.isEmpty()) {
            // if there are no labels, we don't need string formatting
            writer.rustTemplate(
                "#{EndpointPrefix}::new($formatLiteral)",
                "EndpointPrefix" to endpointPrefix
            )
        } else {
            val operationBuildError = OperationBuildError(runtimeConfig)
            writer.rustBlock("") {
                val args = endpointTrait.hostPrefix.labels.map { label ->
                    val member = inputShape.getMember(label.content).get()
                    val field = symbolProvider.toMemberName(member)
                    val invalidFieldError = operationBuildError.invalidField(
                        writer,
                        field,
                        "$field was unset but must be set as part of the endpoint prefix"
                    )
                    if (symbolProvider.toSymbol(member).isOptional()) {
                        rust(
                            """
                        let $field = match $input.$field.as_deref() {
                            Some(field) => field,
                            None => return Err($invalidFieldError.into())
                        };
                       """
                        )
                    } else {
                        rust("let $field = input.$field;")
                    }
                    rust(
                        """
                    if $field.is_empty() {
                        return Err($invalidFieldError.into())
                    }
                    """
                    )
                    (label.content to field)
                }
                writer.rustTemplate(
                    "#{EndpointPrefix}::new(format!($formatLiteral, ${
                    args.joinToString { (member, field) -> "$member = $field" }
                    }))",
                    "EndpointPrefix" to endpointPrefix
                )
            }
        }
    }
}
