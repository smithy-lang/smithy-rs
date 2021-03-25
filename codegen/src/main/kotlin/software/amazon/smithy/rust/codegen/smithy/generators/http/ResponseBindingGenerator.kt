/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.http

import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.outputShape

class ResponseBindingGenerator(protocolConfig: ProtocolConfig, private val operationShape: OperationShape) {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    private val index = HttpBindingIndex.of(model)
    fun renderUpdateOutputBuilder(implBlockWriter: RustWriter): Boolean {
        return implBlockWriter.renderDeserializer()
    }

    fun RustWriter.renderDeserializer(): Boolean {
        val headerUtil = CargoDependency.SmithyHttp(runtimeConfig).asType().member("header")
        val outputShape = operationShape.outputShape(model).builderSymbol(symbolProvider)
        val headerBindings = index.getResponseBindings(operationShape, HttpBinding.Location.HEADER)
        val instant = RuntimeType.Instant(runtimeConfig).toSymbol().rustType()
        // TODO: I think we need to load this from the service shape or configure it via the protocol
        val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS

        if (headerBindings.isEmpty()) {
            return false
        }
        rustBlock(
            "pub fn update_output(output: #1T, _headers: &#2T::HeaderMap) -> Result<#1T, #3T::ParseError>",
            outputShape,
            RuntimeType.http,
            headerUtil
        ) {
            rust("let mut output = output;")

            headerBindings.forEach { binding ->
                val rustMemberName = symbolProvider.toMemberName(binding.member)
                val targetType = model.expectShape(binding.member.target)
                val rustType = symbolProvider.toSymbol(targetType).rustType().stripOuter<RustType.Option>()
                val (coreType, coreShape) = if (targetType is CollectionShape) {
                    rustType.stripOuter<RustType.Container>() to model.expectShape(targetType.member.target)
                } else {
                    rustType to targetType
                }
                val name = safeName()
                if (coreType == instant) {
                    val timestampFormat =
                        index.determineTimestampFormat(
                            binding.member,
                            HttpBinding.Location.HEADER,
                            defaultTimestampFormat
                        )
                    val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                    rust(
                        "let $name: Vec<${coreType.render(true)}> = #T::many_dates(&_headers, ${binding.locationName.dq()}, #T)?;",
                        headerUtil,
                        timestampFormatType
                    )
                } else {
                    rust(
                        "let $name: Vec<${coreType.render(true)}> = #T::read_many(&_headers, ${binding.locationName.dq()})?;",
                        headerUtil
                    )
                    if (coreShape.hasTrait(MediaTypeTrait::class.java)) {
                        rustTemplate(
                            """let $name: Result<Vec<_>, _> = $name
                            .iter().map(|s|
                                #{base_64_decode}(s).map_err(|_|#{header}::ParseError)
                                .and_then(|bytes|String::from_utf8(bytes).map_err(|_|#{header}::ParseError))
                            ).collect();""",
                            "base_64_decode" to RuntimeType.Base64Decode(runtimeConfig),
                            "header" to headerUtil
                        )
                        rust("let $name = $name?;")
                    }
                }
                if (rustType is RustType.Vec) {
                    rust(
                        """
                    if !$name.is_empty() {
                        output = output.$rustMemberName($name.into());
                    }
                    """
                    )
                } else if (rustType is RustType.HashSet) {
                    rust(
                        """
                    if !$name.is_empty() {
                        output = output.$rustMemberName($name.into_iter().collect());
                    }
                    """
                    )
                } else {
                    rustTemplate(
                        """
                        if $name.len() > 1 {
                            return Err(#{header_util}::ParseError)
                        }
                        let mut $name = $name;
                        if let Some(el) = $name.pop() {
                            output = output.$rustMemberName(el);
                        }
                    """,
                        "header_util" to headerUtil
                    )
                }
            }
            rust("Ok(output)")
        }
        return true
    }
}
