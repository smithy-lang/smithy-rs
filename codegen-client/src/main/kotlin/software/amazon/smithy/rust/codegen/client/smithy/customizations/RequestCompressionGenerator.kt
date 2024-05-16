/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.RequestCompressionTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.clientRequestCompression
import software.amazon.smithy.rust.codegen.core.util.getTrait
import java.util.logging.Logger

// Currently, gzip is the only supported encoding.
fun isSupportedEncoding(encoding: String): Boolean = encoding == "gzip"

fun firstSupportedEncoding(encodings: List<String>): String? = encodings.firstOrNull { isSupportedEncoding(it) }

// This generator was implemented based on this spec:
// https://smithy.io/2.0/spec/behavior-traits.html#requestcompression-trait
class RequestCompressionGenerator(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        operationShape.getTrait<RequestCompressionTrait>()?.let { requestCompressionTrait ->
            val logger = Logger.getLogger("SdkSettings")

            if (requestCompressionTrait.encodings.isEmpty()) {
                logger.warning { "No encodings were specified for the requestCompressionTrait on ${operationShape.id}" }
                return emptySection
            }
            // Get the `HttpCompressionTrait`, returning early if this
            // `OperationShape` doesn't have one
            val compressionTrait = operationShape.getTrait<RequestCompressionTrait>() ?: return emptySection
            val encoding = firstSupportedEncoding(compressionTrait.encodings) ?: return emptySection
            // We can remove this once we start supporting other algos.
            // Until then, we shouldn't see anything else coming up here.
            assert(encoding == "gzip") { "Only gzip is supported but encoding was `$encoding`" }
            val runtimeConfig = codegenContext.runtimeConfig
            val compression = clientRequestCompression(runtimeConfig)

            return writable {
                when (section) {
                    is OperationSection.AdditionalRuntimePlugins ->
                        section.addOperationRuntimePlugin(this) {
                            rust("#T::new()", compression.resolve("RequestCompressionRuntimePlugin"))
                        }

                    is OperationSection.AdditionalInterceptors ->
                        section.registerInterceptor(runtimeConfig, this) {
                            rust("#T::new()", compression.resolve("RequestCompressionInterceptor"))
                        }

                    else -> {}
                }
            }
        }

        return emptySection
    }
}
