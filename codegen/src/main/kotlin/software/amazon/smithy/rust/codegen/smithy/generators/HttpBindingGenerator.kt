/*
 * Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *
 * Licensed under the Apache License, Version 2.0 (the "License").
 * You may not use this file except in compliance with the License.
 * A copy of the License is located at
 *
 *  http://aws.amazon.com/apache2.0
 *
 * or in the "license" file accompanying this file. This file is distributed
 * on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either
 * express or implied. See the License for the specific language governing
 * permissions and limitations under the License.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.pattern.SmithyPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.doubleQuote
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.utils.CodeWriter

fun HttpTrait.uriFormatString(): String = uri.segments.map {
    when {
        it.isLabel -> "{${it.content}}"
        else -> it.content
    }
}.joinToString("/", prefix = "/").doubleQuote()

// TODO: TimestampFormat index that

class HttpBindingGenerator(
    val model: Model,
    private val symbolProvider: SymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    private val writer: RustWriter,
    private val shape: OperationShape,
    private val inputShape: StructureShape,
    private val httpTrait: HttpTrait
) {
    // TODO: make abstract
    private val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS
    private val index = HttpBindingIndex(model)
    fun render() {
        writer.rustBlock("impl ${inputShape.id.name}") {
            uriBase(this)
            uriQuery(this)
            httpRequestBuilder(this)
        }
    }

    private fun httpRequestBuilder(writer: RustWriter) {
        writer.rustBlock("pub fn build_http_request(&self, builder: \$T) -> \$T", RuntimeType.Http("request::Builder"), RuntimeType.Http("request::Builder")) {
            write("let mut uri = String::new();")
            write("self.uri_base(&mut uri);")
            if (hasQuery()) {
                write("self.uri_query(&mut uri);")
            }
            write("builder.method(${httpTrait.method.dq()}).uri(uri)")
        }
    }

    /** URI Generation **/

    private fun uriBase(writer: RustWriter) {
        val formatString = httpTrait.uriFormatString()
        val args = httpTrait.uri.labels.map { label ->
            val member = inputShape.getMember(label.content).get()
            "${label.content} = ${labelFmtFun(model.expectShape(member.target), member, label)}"
        }
        val combinedArgs = listOf(formatString, *args.toTypedArray())
        writer.addImport(RuntimeType.StdFmt("Write").toSymbol(), null)
        writer.rustBlock("fn uri_base(&self, output: &mut String)") {
            write("write!(output, ${combinedArgs.joinToString(", ")}).expect(\"formatting should succeed\")")
        }
    }

    private fun hasQuery(): Boolean = index.getRequestBindings(shape, HttpBinding.Location.QUERY).isNotEmpty()

    private fun uriQuery(writer: RustWriter) {
        // Don't bother generating the function if we aren't going to make a query string
        if (!hasQuery()) return
        writer.rustBlock("fn uri_query(&self, output: &mut String)") {
            val queryParams = index.getRequestBindings(shape, HttpBinding.Location.QUERY)
            assert(queryParams.isNotEmpty())
            write("let mut params = Vec::new();")

            queryParams.forEach { param ->
                val memberShape = param.member
                val memberType = model.expectShape(memberShape.target)
                val memberSymbol = symbolProvider.toSymbol(memberShape)
                val memberName = symbolProvider.toMemberName(memberShape)
                OptionIter(memberSymbol, "&self.$memberName") { field ->
                    if (memberType.isListShape) {
                        renderUriList(this, param, memberType.asListShape().get().member, field)
                    } else {
                        write(
                            "params.push((${param.locationName.dq()}, ${
                                paramFmtFun(
                                    memberType,
                                    memberShape,
                                    field
                                )
                            }))"
                        )
                    }
                }
            }
            write("\$T(params, output)", RuntimeType.QueryFormat(runtimeConfig, "write"))
        }
    }

    private fun renderUriList(writer: CodeWriter, param: HttpBinding, innerMember: Shape, memberName: String) {
        val member = param.member
        writer.rustBlock("for inner in $memberName") {
            write("params.push((${param.locationName.dq()}, ${paramFmtFun(innerMember, member, "inner")}))")
        }
    }

    private fun paramFmtFun(target: Shape, member: MemberShape, targetName: String): String {
        return when {
            target.isStringShape -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_string"))
                "$func(&$targetName)"
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.QUERY, defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_timestamp"))
                "$func($targetName, ${writer.format(timestampFormatType)})"
            }
            target.isListShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }
            else -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_default"))
                "$func(&$targetName)"
            }
        }
    }

    private fun labelFmtFun(target: Shape, member: MemberShape, label: SmithyPattern.Segment): String {
        val memberName = symbolProvider.toMemberName(member)
        return when {
            target.isStringShape -> {
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_string"))
                "$func(&self.$memberName, ${label.isGreedyLabel})"
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.LABEL, defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_timestamp"))
                "$func(&self.$memberName, ${writer.format(timestampFormatType)})"
            }
            else -> {
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_default"))
                "$func(self.$memberName)"
            }
        }
    }

    /** End URI generation **/
}
