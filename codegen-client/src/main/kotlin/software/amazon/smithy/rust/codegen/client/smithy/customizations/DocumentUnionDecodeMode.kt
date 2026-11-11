/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docsTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Index
import software.amazon.smithy.rust.codegen.core.smithy.generators.getterName
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Support for the operation-scoped `DocumentUnionDecodeMode`.
 *
 * A client union member that holds documents has two Rust variants, such as `Json(Document)` and
 * `JsonV2(DocumentV2)` (see [DocumentV2Decorator]). A response reads the original variant unless the caller selects
 * `DocumentUnionDecodeMode::V2` for the operation:
 *
 * ```rust
 * client.foo().document_union_decode_mode(DocumentUnionDecodeMode::V2).send().await?;
 * ```
 *
 * The fluent builder stores the selection, and `send()` and its paginator put it in the operation's config bag. The
 * response deserializer loads it and reads the response through `DocumentUnionDecodeMode::read_with`, which makes
 * every nested union see the same mode. The event stream unmarshaller does the same for each event and stream error.
 *
 * Only operations whose output or errors can reach such a union get the API ([documentUnionDecodeModeApplies]).
 */
object DocumentUnionDecodeMode {
    /** The fluent builder's field, setter, and getter, and the paginator's field. */
    const val FIELD = "document_union_decode_mode"
    private const val SETTER = "set_$FIELD"
    private const val GETTER = "get_$FIELD"

    /** The variable that response deserializers bind the operation's mode to. */
    const val BINDING = "document_union_decode_mode"

    fun type(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.smithySchema(runtimeConfig).resolve("serde::DocumentUnionDecodeMode")

    /**
     * Whether reading [operation]'s response depends on the mode: the client has additive `DocumentV2` union
     * variants, and the operation's output, an event in its output stream, or one of its errors can reach one.
     */
    fun applies(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
    ): Boolean {
        if (!documentV2CompanionsEnabled(codegenContext)) {
            return false
        }
        val model = codegenContext.model
        val index = DocumentV2Index.of(model)
        // Event stream errors are part of the operation's errors, so this covers them too.
        return index.reachesUnionVariants(operation.outputShape(model)) ||
            operation.operationErrors(model).any { index.reachesUnionVariants(it) }
    }

    /** Binds [BINDING] to the mode stored in the config bag named [cfg], or the default. */
    fun bind(
        runtimeConfig: RuntimeConfig,
        cfg: String,
    ): Writable =
        writable {
            rustTemplate(
                "let $BINDING = $cfg.load::<#{Mode}>().copied().unwrap_or_default();",
                "Mode" to type(runtimeConfig),
            )
        }

    /**
     * A crate-private function that returns a runtime plugin storing a mode in an operation's config bag. `send()`
     * and paginators add it to the operation's runtime plugins when the caller selected a mode.
     */
    fun plugin(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("document_union_decode_mode_plugin", module) {
            val api = RuntimeType.smithyRuntimeApiClient(runtimeConfig)
            rustTemplate(
                """
                /// Returns a runtime plugin that stores the operation's [`DocumentUnionDecodeMode`](#{Mode}).
                pub(crate) fn document_union_decode_mode_plugin(mode: #{Mode}) -> #{StaticRuntimePlugin} {
                    let mut layer = #{Layer}::new("DocumentUnionDecodeMode");
                    layer.store_put(mode);
                    #{StaticRuntimePlugin}::new().with_config(layer.freeze())
                }
                """,
                "Mode" to type(runtimeConfig),
                "Layer" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::Layer"),
                "StaticRuntimePlugin" to api.resolve("client::runtime_plugin::StaticRuntimePlugin"),
            )
        }

    /**
     * Rebinds [runtimePlugins] with the plugin for [mode], an `Option` expression, when the caller selected a
     * mode.
     */
    fun addPlugin(
        runtimeConfig: RuntimeConfig,
        runtimePlugins: String,
        mode: String,
    ): Writable =
        writable {
            rustTemplate(
                """
                let $runtimePlugins = match $mode {
                    #{Some}(mode) => $runtimePlugins.with_operation_plugin(#{plugin}(mode)),
                    #{None} => $runtimePlugins,
                };
                """,
                *preludeScope,
                "plugin" to plugin(runtimeConfig),
            )
        }

    /**
     * Renders the fluent builder's value method, setter, and getter for the field named [FIELD].
     *
     * Throws a [CodegenException] when one of [operation]'s input members already has a fluent builder method with
     * one of these names.
     */
    fun RustWriter.renderFluentMethods(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
    ) {
        val symbolProvider = codegenContext.symbolProvider
        val input = operation.inputShape(codegenContext.model)
        for (member in input.members()) {
            val memberMethods =
                listOf(symbolProvider.toMemberName(member).removePrefix("r##"), member.setterName(), member.getterName())
            listOf(FIELD, SETTER, GETTER).firstOrNull { it in memberMethods }?.also { name ->
                throw CodegenException(
                    "`${operation.id}` needs a fluent builder method named `$name` to select its " +
                        "`DocumentUnionDecodeMode`, but input member `${member.memberName}` already has a method " +
                        "with that name.",
                )
            }
        }

        val scope =
            arrayOf(
                *preludeScope,
                "Mode" to type(codegenContext.runtimeConfig),
            )
        docsTemplate(
            """
            Sets how this operation's response reads union members that hold documents.

            This operation's response holds unions with a member whose value holds documents. Such a member has two
            variants, such as `Json(Document)` and `JsonV2(DocumentV2)`. With
            [`DocumentUnionDecodeMode::V1`](#{Mode}::V1), the default, the response uses the original variant. With
            [`DocumentUnionDecodeMode::V2`](#{Mode}::V2), it uses the `DocumentV2` variant for every such member,
            including a value that a `Document` could hold. The mode applies to the whole response, including nested
            unions, modeled errors, and the events of an event stream. Requests accept either variant regardless of the
            mode.
            """.trimIndent(),
            *scope,
        )
        rustTemplate(
            """
            pub fn $FIELD(mut self, input: #{Mode}) -> Self {
                self.$FIELD = #{Some}(input);
                self
            }

            /// Sets or clears how this operation's response reads union members that hold documents.
            ///
            /// `None` uses the default, [`DocumentUnionDecodeMode::V1`](#{Mode}::V1). See
            /// [`$FIELD`](Self::$FIELD).
            pub fn $SETTER(mut self, input: #{Option}<#{Mode}>) -> Self {
                self.$FIELD = input;
                self
            }

            /// Returns how this operation's response reads union members that hold documents, if it was set. See
            /// [`$FIELD`](Self::$FIELD).
            pub fn $GETTER(&self) -> &#{Option}<#{Mode}> {
                &self.$FIELD
            }
            """,
            *scope,
        )
    }

    private val module = RustModule.private("document_union_decode_mode")
}
