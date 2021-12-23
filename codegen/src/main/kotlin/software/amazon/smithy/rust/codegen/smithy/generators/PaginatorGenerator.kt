/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.PaginatedIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase

class PaginatorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    service: ServiceShape,
    operation: OperationShape,
    private val generics: ClientGenerics
) {
    private val paginatorName = "${operation.id.name.toPascalCase()}Paginator"
    private val runtimeConfig = symbolProvider.config().runtimeConfig
    private val idx = PaginatedIndex.of(model)
    private val paginationInfo =
        idx.getPaginationInfo(service, operation).orNull() ?: PANIC("failed to load pagination info")
    private val module = RustModule(
        "paginator",
        RustMetadata(public = true),
        documentation = "Paginators for the service"
    )

    private val outputType = operation.outputShape(model)

    fun paginatorType(): RuntimeType = RuntimeType.forInlineFun(
        paginatorName,
        module,
        generate()
    )

    private val codegenScope = arrayOf(
        "generics" to generics.decl,
        "bounds" to generics.bounds,
        "page_size_setter" to pageSizeSetter(),

        // Operation Types
        "operation" to symbolProvider.toSymbol(operation),
        "Input" to symbolProvider.toSymbol(operation.inputShape(model)),
        "Output" to symbolProvider.toSymbol(operation.outputShape(model)),
        "Error" to operation.errorSymbol(symbolProvider),
        "Builder" to operation.inputShape(model).builderSymbol(symbolProvider),

        // SDK Types
        "SdkError" to CargoDependency.SmithyHttp(runtimeConfig).asType()
            .copy(name = "result::SdkError"),
        "client" to CargoDependency.SmithyClient(runtimeConfig).asType(),
        "fn_stream" to CargoDependency.SmithyAsync(runtimeConfig).asType().member("future::fn_stream"),

        // External Types
        "Stream" to CargoDependency.TokioStream.asType().member("Stream")

    )

    /** Generate the paginator struct & impl **/
    private fun generate() = writable {
        val outputTokenLens = LensGenerator(model, symbolProvider).generateReferentialLens(
            outputType,
            paginationInfo.outputTokenMemberPath
        )
        val inputTokenMember = symbolProvider.toMemberName(paginationInfo.inputTokenMember)
        rustTemplate(
            """
            /// Paginator for #{operation:D}
            pub struct $paginatorName#{generics:W} {
                handle: std::sync::Arc<crate::client::Handle${generics.inst}>,
                builder: #{Builder}
            }

            impl ${generics.inst}${paginatorName}${generics.inst} where #{bounds:W} {
                /// Create a new paginator-wrapper
                pub(crate) fn new(handle: std::sync::Arc<crate::client::Handle${generics.inst}>, builder: #{Builder}) -> Self {
                    Self {
                        handle,
                        builder,
                    }
                }

                #{page_size_setter:W}

                #{items_fn:W}


                /// Create the pagination stream
                ///
                /// _Note:_ No requests will be dispatched until the stream is used (eg. with [`.next()`](tokio_stream::StreamExt::next)).
                pub async fn send(self) -> impl #{Stream}<Item = std::result::Result<#{Output}, #{SdkError}<#{Error}>>> + Unpin
                where
                    R::Policy: #{client}::bounds::SmithyRetryPolicy<
                        #{Input}OperationOutputAlias,
                        #{Output},
                        #{Error},
                        #{Input}OperationRetryAlias
                    >, {
                    // Move individual fields out of self for the borrow checker
                    let builder = self.builder;
                    let handle = self.handle;
                    #{fn_stream}::FnStream::new(move |tx| Box::pin(async move {
                        // Build the input for the first time. If required fields are missing, this is where we'll produce an early error.
                        let mut input = match builder.build().map_err(|err|#{SdkError}::ConstructionFailure(err.into())) {
                            Ok(input) => input,
                            Err(e) =>  { let _ = tx.send(Err(e)).await; return; }
                        };
                        loop {
                            let op = match input.make_operation(&handle.conf)
                                .await
                                .map_err(|err|#{SdkError}::ConstructionFailure(err.into())) {
                                Ok(op) => op,
                                Err(e) => {
                                    let _ = tx.send(Err(e)).await;
                                    return;
                                }
                            };
                            let resp = handle.client.call(op).await;
                            // If the input member is None or it was an error
                            let done = match resp {
                                Ok(ref resp) => {
                                    input.$inputTokenMember = #{output_token}(resp).cloned();
                                    input.$inputTokenMember.is_none()
                                },
                                Err(_) => true,
                            };
                            if let Err(_) = tx.send(resp).await {
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
            "output_token" to outputTokenLens
        )
    }

    /** Type of the inner item of the paginator */
    private fun itemType(): String {
        val members = paginationInfo.itemsMemberPath
        val type = symbolProvider.toSymbol(model.expectShape(members.last().target)).rustType()
        check(type is RustType.Vec || type is RustType.HashMap)
        return (type as RustType.Container).member.render(true)
    }

    /** Generate a `.items()` function to expose flattened pagination when modeled */
    private fun itemsFn(): Writable = writable {
        itemsPaginator()?.also { itemPaginatorType ->
            val documentedPath = paginationInfo.itemsMemberPath.joinToString(".") { symbolProvider.toMemberName(it) }
            rustTemplate(
                """
                /// Create a flattened paginator
                ///
                /// This paginator automatically flattens results using `$documentedPath`. Queries to the underlying service
                /// are dispatched lazily.
                pub fn items(
                        self
                    ) -> #{ItemPaginator}${generics.inst} {
                        #{ItemPaginator}(self)
                    }
                """,
                "ItemPaginator" to itemPaginatorType
            )
        }
    }

    /** Generate a struct with a `items()` method that flattens the paginator **/
    private fun itemsPaginator(): RuntimeType? = if (paginationInfo.itemsMemberPath.isEmpty()) {
        null
    } else {
        RuntimeType.forInlineFun("${paginatorName}Items", module) {
            it.rustTemplate(
                """
                /// Flattened paginator for for $paginatorName
                ///
                /// This is created with [`.items()`]($paginatorName::items)
                pub struct ${paginatorName}Items#{generics:W}($paginatorName${generics.inst});

                impl ${generics.inst} ${paginatorName}Items${generics.inst} where #{bounds:W} {
                    /// Create the pagination stream
                    ///
                    /// _Note: No requests will be dispatched until the stream is used (eg. with [`.next()`](tokio_stream::StreamExt::next))._
                    ///
                    /// To read the entirety of the paginator, use [`.collect::<Result<Vec<_>, _>()`](tokio_stream::StreamExt::collect).
                    pub async fn send(self) -> impl #{Stream}<Item = std::result::Result<${itemType()}, #{SdkError}<#{Error}>>> + Unpin
                    where
                        R::Policy: #{client}::bounds::SmithyRetryPolicy<
                            #{Input}OperationOutputAlias,
                            #{Output},
                            #{Error},
                            #{Input}OperationRetryAlias
                        >, {
                        #{fn_stream}::TryFlatMap::new(self.0.send().await).flat_map(|page|#{extract_items}(page).unwrap_or_default().into_iter())
                    }
                }

                """,
                "extract_items" to LensGenerator(model, symbolProvider).generateOwningLens(
                    outputType,
                    paginationInfo.itemsMemberPath
                ),
                *codegenScope
            )
        }
    }

    private fun pageSizeSetter() = writable {
        paginationInfo.pageSizeMember.orNull()?.also {
            val memberName = symbolProvider.toMemberName(it)
            val pageSizeT = symbolProvider.toSymbol(it).rustType().stripOuter<RustType.Option>().render(true)
            rust(
                """
                /// Set the page size
                pub fn page_size(mut self, limit: $pageSizeT) -> Self {
                    self.builder.$memberName = Some(limit);
                    self
                }
                """
            )
        }
    }
}
