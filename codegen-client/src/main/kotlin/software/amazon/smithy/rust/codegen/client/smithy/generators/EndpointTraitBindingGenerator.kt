/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.rustFormatString
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.AllowUninlinedFormatArgs
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.util.inputShape

fun EndpointTrait.prefixFormatString(): String {
    return this.hostPrefix.rustFormatString("", "")
}

class EndpointTraitBindings(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    operationShape: OperationShape,
    private val endpointTrait: EndpointTrait,
) {
    private val inputShape = operationShape.inputShape(model)
    private val endpointPrefix = RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::endpoint::EndpointPrefix")

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
    fun render(
        writer: RustWriter,
        input: String,
        generateValidation: Boolean = true,
    ) {
        // the Rust format pattern to make the endpoint prefix e.g. "{}.foo"
        val formatLiteral = endpointTrait.prefixFormatString()
        if (endpointTrait.hostPrefix.labels.isEmpty()) {
            // if there are no labels, we don't need string formatting
            writer.rustTemplate(
                "#{EndpointPrefix}::new($formatLiteral)",
                "EndpointPrefix" to endpointPrefix,
            )
        } else {
            writer.rustBlock("") {
                // build a list of args: `labelname = "field"`
                // these eventually end up in the format! macro invocation:
                // ```format!("some.{endpoint}", endpoint = endpoint);```
                val args =
                    endpointTrait.hostPrefix.labels.map { label ->
                        val memberShape = inputShape.getMember(label.content).get()
                        val field = symbolProvider.toMemberName(memberShape)
                        if (symbolProvider.toSymbol(memberShape).isOptional()) {
                            rust("let $field = $input.$field.as_deref().unwrap_or_default();")
                        } else {
                            // NOTE: this is dead code until we start respecting @required
                            rust("let $field = &$input.$field;")
                        }
                        if (generateValidation) {
                            val errorString = "$field was unset or empty but must be set as part of the endpoint prefix"
                            val contents =
                                """
                                if $field.is_empty() {
                                    return Err(#{InvalidEndpointError}::failed_to_construct_uri("$errorString").into());
                                }
                                """
                            rustTemplate(
                                contents,
                                "InvalidEndpointError" to
                                    RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                        .resolve("client::endpoint::error::InvalidEndpointError"),
                            )
                        }
                        "${label.content} = $field"
                    }
                // Suppress the suggestion that would change the following:
                //   EndpointPrefix::new(format!("{accountId}."))
                // To:
                //   EndpointPrefix::new(format!("{accountId}.", accountId = account_id))
                AllowUninlinedFormatArgs.render(this)
                rustTemplate(
                    "#{EndpointPrefix}::new(format!($formatLiteral, ${args.joinToString()}))",
                    "EndpointPrefix" to endpointPrefix,
                )
            }
        }
    }
}
