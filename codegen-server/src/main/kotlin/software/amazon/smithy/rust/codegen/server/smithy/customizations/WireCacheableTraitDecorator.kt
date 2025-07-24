/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.traits.CacheableTrait
import kotlin.streams.asSequence

/**
 * Decorator that adds WireCacheable trait implementations to structures that are targeted by @cacheable members.
 */
class WireCacheableTraitDecorator : ServerCodegenDecorator {
    override val name: String = "WireCacheableTraitDecorator"
    override val order: Byte = 0

    override fun structureCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> {
        return baseCustomizations + WireCacheableTraitCustomization(codegenContext)
    }
}

/**
 * Customization that adds WireCacheable trait implementation to structures that are targeted by @cacheable members.
 */
class WireCacheableTraitCustomization(private val codegenContext: ServerCodegenContext) : StructureCustomization() {
    override fun section(section: StructureSection): Writable {
        return when (section) {
            is StructureSection.AdditionalTraitImpls -> {
                if (shouldImplementWireCacheable(section.shape)) {
                    generateWireCacheableImpl(section.shape, section.structName)
                } else {
                    writable { }
                }
            }

            else -> writable { }
        }
    }

    /**
     * Check if this structure should implement WireCacheable.
     * A structure should implement WireCacheable if it's targeted by any @cacheable member.
     */
    private fun shouldImplementWireCacheable(shape: StructureShape): Boolean {
        val model = codegenContext.model

        // Find all members in the model that have the @cacheable trait
        return model.shapes()
            .asSequence()
            .filterIsInstance<MemberShape>()
            .filter { it.hasTrait(CacheableTrait::class.java) }
            .any { member ->
                // Check if this member targets our structure shape
                val targetShape = model.expectShape(member.target)
                targetShape.id == shape.id
            }
    }

    /**
     * Generate the WireCacheable trait implementation for the given structure.
     */
    private fun generateWireCacheableImpl(
        shape: StructureShape,
        structName: String,
    ): Writable {
        return writable {
            val runtimeConfig = codegenContext.runtimeConfig
            val cacheableModule = RuntimeType.cacheable()

            // Use the existing serializer function that should be generated
            val serializerFnName =
                "crate::protocol_serde::shape_${shape.id.name.lowercase()}::ser_${shape.id.name.lowercase()}"

            rustTemplate(
                """
                impl #{WireCacheable} for $structName {
                    /// Serialize this value to CBOR bytes for caching
                    fn to_bytes(&self) -> #{Bytes} {
                        let buffer = Vec::new();
                        let mut encoder = #{CborEncoder}::new(buffer);

                        // Delegate to the existing CBOR serializer for this shape
                        $serializerFnName(&mut encoder, self)
                            .expect("serialization should be infallible");

                        #{Bytes}::from(encoder.into_writer())
                    }

                    /// Validate serialized CBOR bytes for this shape
                    fn validate(_bytes: &[u8]) -> ::std::result::Result<(), #{ValidationError}> {
                        // TODO(wireCaching): This need to actually generate a bespoke deserializer for this shape.
                        Ok(())
                    }
                }
                """,
                "WireCacheable" to cacheableModule.resolve("WireCacheable"),
                "ValidationError" to cacheableModule.resolve("ValidationError"),
                "Bytes" to RuntimeType.Bytes,
                "CborEncoder" to RuntimeType.smithyCbor(runtimeConfig).resolve("Encoder"),
                "CborDecoder" to RuntimeType.smithyCbor(runtimeConfig).resolve("Decoder"),
            )
        }
    }
}
