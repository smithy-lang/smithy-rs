/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentBuilderConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentBuilderGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.waiters.WaitableTrait
import software.amazon.smithy.waiters.Waiter

/**
 * Generates waiters for the Smithy @waitable trait.
 *
 * This will place waiter-specific fluent builders into individual waiter submodules of the `crate::waiter` module,
 * and place a `Waiters` trait in the client module that can be imported to initiate waiters.
 */
class WaitableGenerator(
    private val codegenContext: ClientCodegenContext,
    allOperations: List<OperationShape>,
) {
    private data class WaitableOp(
        val shape: OperationShape,
        val waiters: List<WaiterSpec>,
    )

    private data class WaiterSpec(
        val waiterName: String,
        val waiter: Waiter,
        val fluentBuilder: RuntimeType,
    )

    private val operations =
        allOperations.mapNotNull { op ->
            op.getTrait<WaitableTrait>()?.let {
                WaitableOp(
                    op,
                    it.waiters.entries.map { (name, waiter) ->
                        WaiterSpec(
                            name,
                            waiter,
                            waiterFluentBuilder(name, waiter, op),
                        )
                    },
                )
            }
        }
            .sortedBy { it.shape.id }

    fun render(crate: RustCrate) {
        if (operations.isEmpty()) {
            return
        }

        crate.withModule(ClientRustModule.client) {
            docs(
                """
                Waiter functions for the client.

                Import this trait to get `wait_until` methods on the client.
                """,
            )
            rustBlockTemplate("pub trait Waiters") {
                for (op in operations) {
                    for (spec in op.waiters) {
                        val waiterDocs = spec.waiter.documentation.orNull() ?: "Wait for `${spec.waiterName.toSnakeCase()}`"
                        docs(waiterDocs)
                        renderWaiterFnDeclaration(spec)
                        rust(";")
                    }
                }
            }

            rustBlockTemplate("impl Waiters for Client") {
                for (op in operations) {
                    for (spec in op.waiters) {
                        renderWaiterFnDeclaration(spec)
                        rustTemplate(
                            "{ #{FluentBuilder}::new(self.handle.clone()) }",
                            "FluentBuilder" to spec.fluentBuilder,
                        )
                    }
                }
            }
        }
    }

    private fun RustWriter.renderWaiterFnDeclaration(spec: WaiterSpec) {
        val fnName = "wait_until_${spec.waiterName.toSnakeCase()}"
        rustTemplate(
            "fn $fnName(&self) -> #{FluentBuilder}",
            "FluentBuilder" to spec.fluentBuilder,
        )
    }

    private fun waiterModule(waiterName: String): RustModule =
        RustModule.public(
            RustReservedWords.escapeIfNeeded(waiterName.toSnakeCase(), EscapeFor.ModuleName),
            ClientRustModule.waiters,
            documentationOverride = "Supporting types for the `${waiterName.toSnakeCase()}` waiter.",
        )

    private fun waiterFluentBuilder(
        waiterName: String,
        waiter: Waiter,
        operation: OperationShape,
    ): RuntimeType {
        val builderName = "${waiterName.toPascalCase()}FluentBuilder"
        val waiterModule = waiterModule(waiterName)
        return RuntimeType.forInlineFun(builderName, waiterModule) {
            FluentBuilderGenerator(
                codegenContext,
                operation,
                builderName = builderName,
                config = WaiterFluentBuilderConfig(codegenContext, operation, waiter, waiterName, waiterModule),
            ).render(this)
        }
    }
}

private class WaiterFluentBuilderConfig(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
    private val waiter: Waiter,
    private val waiterName: String,
    private val waiterModule: RustModule,
) : FluentBuilderConfig {
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    private val scope =
        arrayOf(
            *preludeScope,
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "Duration" to RuntimeType.Duration,
            "Error" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors::context::Error"),
            "FinalPoll" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::waiters::FinalPoll"),
            "HttpResponse" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::orchestrator::HttpResponse"),
            "Operation" to symbolProvider.toSymbol(operation),
            "OperationError" to symbolProvider.symbolForOperationError(operation),
            "OperationOutput" to symbolProvider.toSymbol(operation.outputShape(model)),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "WaiterError" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::waiters::error::WaiterError"),
            "WaiterOrchestrator" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::waiters::WaiterOrchestrator"),
            "attach_waiter_tracing_span" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::waiters::attach_waiter_tracing_span"),
        )

    override fun includeConfigOverride(): Boolean = false

    override fun includePaginators(): Boolean = false

    override fun documentBuilder(): Writable =
        writable {
            docs(
                """
                Fluent builder for the `${waiterName.toSnakeCase()}` waiter.

                This builder is intended to be used similar to the other fluent builders for
                normal operations on the client. However, instead of a `send` method, it has
                a `wait` method that takes a maximum amount of time to wait.

                Construct this fluent builder using the client by importing the
                [`Waiters`](crate::client::Waiters) trait and calling the methods
                prefixed with `wait_until`.
                """,
            )
        }

    override fun sendMethods(): Writable =
        writable {
            val waiterDocs = waiter.documentation.orNull() ?: "Wait for `${waiterName.toSnakeCase()}`"
            docs(waiterDocs)
            rustTemplate(
                """
                pub async fn wait(self, max_wait: #{Duration}) -> #{Result}<#{FinalPollAlias}, #{WaiterErrorAlias}> {
                    let input = self.inner.build()
                        .map_err(#{WaiterError}::construction_failure)?;
                    let runtime_plugins = #{Operation}::operation_runtime_plugins(
                        self.handle.runtime_plugins.clone(),
                        &self.handle.conf,
                        #{None},
                    ).with_operation_plugin(#{WaiterFeatureTrackerRuntimePlugin}::new());
                    let mut cfg = #{ConfigBag}::base();
                    let runtime_components_builder = runtime_plugins.apply_client_configuration(&mut cfg)
                        .map_err(#{WaiterError}::construction_failure)?;
                    let time_components = runtime_components_builder.into_time_components();
                    let sleep_impl = time_components.sleep_impl().expect("a sleep impl is required by waiters");
                    let time_source = time_components.time_source().expect("a time source is required by waiters");

                    #{acceptor}
                    let operation = move || {
                        let input = input.clone();
                        let runtime_plugins = runtime_plugins.clone();
                        async move {
                            #{Operation}::orchestrate(&runtime_plugins, input).await
                        }
                    };
                    let orchestrator = #{WaiterOrchestrator}::builder()
                        .min_delay(#{Duration}::from_secs(${waiter.minDelay}))
                        .max_delay(#{Duration}::from_secs(${waiter.maxDelay}))
                        .max_wait(max_wait)
                        .time_source(time_source)
                        .sleep_impl(sleep_impl)
                        .acceptor(acceptor)
                        .operation(operation)
                        .build();
                    #{attach_waiter_tracing_span}(orchestrator.orchestrate()).await
                }
                """,
                *scope,
                "acceptor" to
                    writable {
                        WaiterAcceptorGenerator(codegenContext, operation, waiter, "input").render(this)
                    },
                "FinalPollAlias" to finalPollTypeAlias(),
                "WaiterErrorAlias" to waiterErrorTypeAlias(),
                "WaiterFeatureTrackerRuntimePlugin" to
                    RuntimeType.forInlineDependency(
                        InlineDependency.sdkFeatureTracker(runtimeConfig),
                    ).resolve("waiter::WaiterFeatureTrackerRuntimePlugin"),
            )
        }

    private fun finalPollTypeAlias(): RuntimeType =
        "${waiterName.toPascalCase()}FinalPoll".let { name ->
            RuntimeType.forInlineFun(name, waiterModule) {
                docs("Successful return type for the `${waiterName.toSnakeCase()}` waiter.")
                rustTemplate(
                    "pub type $name = #{FinalPoll}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>>;",
                    *scope,
                )
            }
        }

    private fun waiterErrorTypeAlias(): RuntimeType =
        "WaitUntil${waiterName.toPascalCase()}Error".let { name ->
            RuntimeType.forInlineFun(name, waiterModule) {
                docs("Error type for the `${waiterName.toSnakeCase()}` waiter.")
                rustTemplate("pub type $name = #{WaiterError}<#{OperationOutput}, #{OperationError}>;", *scope)
            }
        }
}
