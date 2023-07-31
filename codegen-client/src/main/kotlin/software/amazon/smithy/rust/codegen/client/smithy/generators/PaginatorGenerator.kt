/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.PaginatedIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.model.traits.PaginatedTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.findMemberWithTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

// TODO(https://github.com/awslabs/smithy-rs/issues/1013) Support pagination when the idempotency trait is present
fun OperationShape.isPaginated(model: Model) =
    hasTrait<PaginatedTrait>() && inputShape(model)
        .findMemberWithTrait<IdempotencyTokenTrait>(model) == null

class PaginatorGenerator private constructor(
    private val codegenContext: ClientCodegenContext,
    operation: OperationShape,
    private val generics: FluentClientGenerics,
    retryClassifier: RuntimeType,
) {
    companion object {
        fun paginatorType(
            codegenContext: ClientCodegenContext,
            generics: FluentClientGenerics,
            operationShape: OperationShape,
            retryClassifier: RuntimeType,
        ): RuntimeType? {
            return if (operationShape.isPaginated(codegenContext.model)) {
                PaginatorGenerator(
                    codegenContext,
                    operationShape,
                    generics,
                    retryClassifier,
                ).paginatorType()
            } else {
                null
            }
        }
    }

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val paginatorName = "${operation.id.name.toPascalCase()}Paginator"
    private val idx = PaginatedIndex.of(model)
    private val paginationInfo = idx.getPaginationInfo(codegenContext.serviceShape, operation).orNull()
        ?: PANIC("failed to load pagination info")
    private val module = RustModule.public(
        "paginator",
        parent = symbolProvider.moduleForShape(operation),
        documentationOverride = "Paginator for this operation",
    )

    private val inputType = symbolProvider.toSymbol(operation.inputShape(model))
    private val outputShape = operation.outputShape(model)
    private val outputType = symbolProvider.toSymbol(outputShape)
    private val errorType = symbolProvider.symbolForOperationError(operation)

    private fun paginatorType(): RuntimeType = RuntimeType.forInlineFun(
        paginatorName,
        module,
        generate(),
    )

    private val codegenScope = arrayOf(
        *preludeScope,
        "generics" to generics.decl,
        "bounds" to generics.bounds,
        "page_size_setter" to pageSizeSetter(),
        "send_bounds" to generics.sendBounds(
            symbolProvider.toSymbol(operation),
            outputType,
            errorType,
            retryClassifier,
        ),

        // Operation Types
        "operation" to symbolProvider.toSymbol(operation),
        "Input" to inputType,
        "Output" to outputType,
        "Error" to errorType,
        "Builder" to symbolProvider.symbolForBuilder(operation.inputShape(model)),

        // SDK Types
        "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::orchestrator::HttpResponse"),
        "SdkError" to RuntimeType.sdkError(runtimeConfig),
        "client" to RuntimeType.smithyClient(runtimeConfig),
        "fn_stream" to RuntimeType.smithyAsync(runtimeConfig).resolve("future::fn_stream"),

        // External Types
        "Stream" to RuntimeType.TokioStream.resolve("Stream"),

    )

    /** Generate the paginator struct & impl **/
    private fun generate() = writable {
        val outputTokenLens = NestedAccessorGenerator(codegenContext).generateBorrowingAccessor(
            outputShape,
            paginationInfo.outputTokenMemberPath,
        )
        val inputTokenMember = symbolProvider.toMemberName(paginationInfo.inputTokenMember)
        rustTemplate(
            """
            /// Paginator for #{operation:D}
            pub struct $paginatorName#{generics:W} {
                handle: std::sync::Arc<crate::client::Handle${generics.inst}>,
                builder: #{Builder},
                stop_on_duplicate_token: bool,
            }

            impl${generics.inst} ${paginatorName}${generics.inst} #{bounds:W} {
                /// Create a new paginator-wrapper
                pub(crate) fn new(handle: std::sync::Arc<crate::client::Handle${generics.inst}>, builder: #{Builder}) -> Self {
                    Self {
                        handle,
                        builder,
                        stop_on_duplicate_token: true,
                    }
                }

                #{page_size_setter:W}

                #{items_fn:W}

                /// Stop paginating when the service returns the same pagination token twice in a row.
                ///
                /// Defaults to true.
                ///
                /// For certain operations, it may be useful to continue on duplicate token. For example,
                /// if an operation is for tailing a log file in real-time, then continuing may be desired.
                /// This option can be set to `false` to accommodate these use cases.
                pub fn stop_on_duplicate_token(mut self, stop_on_duplicate_token: bool) -> Self {
                    self.stop_on_duplicate_token = stop_on_duplicate_token;
                    self
                }

                /// Create the pagination stream
                ///
                /// _Note:_ No requests will be dispatched until the stream is used (eg. with [`.next().await`](tokio_stream::StreamExt::next)).
                pub fn send(self) -> impl #{Stream}<Item = #{item_type}> + #{Unpin}
                #{send_bounds:W} {
                    // Move individual fields out of self for the borrow checker
                    let builder = self.builder;
                    let handle = self.handle;
                    #{runtime_plugin_init}
                    #{fn_stream}::FnStream::new(move |tx| #{Box}::pin(async move {
                        // Build the input for the first time. If required fields are missing, this is where we'll produce an early error.
                        let mut input = match builder.build().map_err(#{SdkError}::construction_failure) {
                            #{Ok}(input) => input,
                            #{Err}(e) => { let _ = tx.send(#{Err}(e)).await; return; }
                        };
                        loop {
                            let resp = #{orchestrate};
                            // If the input member is None or it was an error
                            let done = match resp {
                                #{Ok}(ref resp) => {
                                    let new_token = #{output_token}(resp);
                                    let is_empty = new_token.map(|token| token.is_empty()).unwrap_or(true);
                                    if !is_empty && new_token == input.$inputTokenMember.as_ref() && self.stop_on_duplicate_token {
                                        true
                                    } else {
                                        input.$inputTokenMember = new_token.cloned();
                                        is_empty
                                    }
                                },
                                #{Err}(_) => true,
                            };
                            if tx.send(resp).await.is_err() {
                                // receiving end was dropped
                                return
                            }
                            if done {
                                return
                            }
                        }
                    }))
                }
            }
            """,
            *codegenScope,
            "items_fn" to itemsFn(),
            "output_token" to outputTokenLens,
            "item_type" to writable {
                if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                    rustTemplate("#{Result}<#{Output}, #{SdkError}<#{Error}>>", *codegenScope)
                } else {
                    rustTemplate("#{Result}<#{Output}, #{SdkError}<#{Error}, #{HttpResponse}>>", *codegenScope)
                }
            },
            "orchestrate" to writable {
                if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                    rustTemplate(
                        """
                        {
                            let op = match input.make_operation(&handle.conf)
                                .await
                                .map_err(#{SdkError}::construction_failure) {
                                #{Ok}(op) => op,
                                #{Err}(e) => {
                                    let _ = tx.send(#{Err}(e)).await;
                                    return;
                                }
                            };
                            handle.client.call(op).await
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        "#{operation}::orchestrate(&runtime_plugins, input.clone()).await",
                        *codegenScope,
                    )
                }
            },
            "runtime_plugin_init" to writable {
                if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        let runtime_plugins = #{operation}::operation_runtime_plugins(
                            handle.runtime_plugins.clone(),
                            &handle.conf,
                            #{None},
                        );
                        """,
                        *codegenScope,
                        "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                    )
                }
            },
        )
    }

    /** Type of the inner item of the paginator */
    private fun itemType(): String {
        val members = paginationInfo.itemsMemberPath
        val type = symbolProvider.toSymbol(model.expectShape(members.last().target)).rustType()
        check(type is RustType.Vec || type is RustType.HashMap)
        return when (type) {
            is RustType.Vec -> type.member.render(true)
            is RustType.HashMap -> "(${type.key.render(true)}, ${type.member.render(true)})"
            else -> PANIC("only HashMaps or Vecs may be used for item pagination.")
        }
    }

    /** Generate an `.items()` function to expose flattened pagination when modeled */
    private fun itemsFn(): Writable =
        writable {
            itemsPaginator()?.also { itemPaginatorType ->
                val documentedPath =
                    paginationInfo.itemsMemberPath.joinToString(".") { symbolProvider.toMemberName(it) }
                rustTemplate(
                    """
                    /// Create a flattened paginator
                    ///
                    /// This paginator automatically flattens results using `$documentedPath`. Queries to the underlying service
                    /// are dispatched lazily.
                    pub fn items(self) -> #{ItemPaginator}${generics.inst} {
                        #{ItemPaginator}(self)
                    }
                    """,
                    "ItemPaginator" to itemPaginatorType,
                )
            }
        }

    /** Generate a struct with a `items()` method that flattens the paginator **/
    private fun itemsPaginator(): RuntimeType? = if (paginationInfo.itemsMemberPath.isEmpty()) {
        null
    } else {
        RuntimeType.forInlineFun("${paginatorName}Items", module) {
            rustTemplate(
                """
                /// Flattened paginator for `$paginatorName`
                ///
                /// This is created with [`.items()`]($paginatorName::items)
                pub struct ${paginatorName}Items#{generics:W}($paginatorName${generics.inst});

                impl ${generics.inst} ${paginatorName}Items${generics.inst} #{bounds:W} {
                    /// Create the pagination stream
                    ///
                    /// _Note: No requests will be dispatched until the stream is used (eg. with [`.next().await`](tokio_stream::StreamExt::next))._
                    ///
                    /// To read the entirety of the paginator, use [`.collect::<Result<Vec<_>, _>()`](tokio_stream::StreamExt::collect).
                    pub fn send(self) -> impl #{Stream}<Item = #{item_type}> + #{Unpin}
                    #{send_bounds:W} {
                        #{fn_stream}::TryFlatMap::new(self.0.send()).flat_map(|page| #{extract_items}(page).unwrap_or_default().into_iter())
                    }
                }

                """,
                "extract_items" to NestedAccessorGenerator(codegenContext).generateOwnedAccessor(
                    outputShape,
                    paginationInfo.itemsMemberPath,
                ),
                "item_type" to writable {
                    if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                        rustTemplate("#{Result}<${itemType()}, #{SdkError}<#{Error}>>", *codegenScope)
                    } else {
                        rustTemplate("#{Result}<${itemType()}, #{SdkError}<#{Error}, #{HttpResponse}>>", *codegenScope)
                    }
                },
                *codegenScope,
            )
        }
    }

    private fun pageSizeSetter() = writable {
        paginationInfo.pageSizeMember.orNull()?.also {
            val memberName = symbolProvider.toMemberName(it)
            val pageSizeT =
                symbolProvider.toSymbol(it).rustType().stripOuter<RustType.Option>().render(true)
            rustTemplate(
                """
                /// Set the page size
                ///
                /// _Note: this method will override any previously set value for `$memberName`_
                pub fn page_size(mut self, limit: $pageSizeT) -> Self {
                    self.builder.$memberName = #{Some}(limit);
                    self
                }
                """,
                *preludeScope,
            )
        }
    }
}
