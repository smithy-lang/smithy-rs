/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDocs
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.docsTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProviderContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Modules for code generated client crates.
 */
object ClientRustModule {
    /** crate */
    val root = RustModule.LibRs

    /** crate::client */
    val client = Client.self

    object Client {
        /** crate::client */
        val self = RustModule.public("client")

        /** crate::client::customize */
        val customize = RustModule.public("customize", parent = self)
    }

    /** crate::config */
    val config = Config.self

    object Config {
        /** crate::client */
        val self = RustModule.public("config")

        /** crate::config::auth */
        val auth = RustModule.public("auth", parent = self)

        /** crate::config::endpoint */
        val endpoint = RustModule.public("endpoint", parent = self)

        /** crate::config::http */
        val http = RustModule.public("http", parent = self)

        /** crate::config::interceptors */
        val interceptors = RustModule.public("interceptors", parent = self)

        /** crate::config::retry */
        val retry = RustModule.public("retry", parent = self)

        /** crate::config::timeout */
        val timeout = RustModule.public("timeout", parent = self)
    }

    val Error = RustModule.public("error")
    val Operation = RustModule.public("operation")
    val Meta = RustModule.public("meta")
    val Input = RustModule.public("input")
    val Output = RustModule.public("output")

    /** crate::primitives */
    val primitives = Primitives.self

    object Primitives {
        /** crate::primitives */
        val self = RustModule.public("primitives")

        /** crate::primitives::event_stream */
        val EventStream = RustModule.public("event_stream", parent = self)
    }

    /** crate::types */
    val types = Types.self

    object Types {
        /** crate::types */
        val self = RustModule.public("types")

        /** crate::types::error */
        val Error = RustModule.public("error", parent = self)
    }

    val waiters = RustModule.public("waiters")
}

class ClientModuleDocProvider(
    private val codegenContext: ClientCodegenContext,
    private val serviceName: String,
) : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        val strDoc: (String) -> Writable = { str -> writable { docs(escape(str)) } }
        return when (module) {
            ClientRustModule.client -> clientModuleDoc()
            ClientRustModule.Client.customize -> customizeModuleDoc()
            ClientRustModule.config -> strDoc("Configuration for $serviceName.")
            ClientRustModule.Config.auth -> strDoc("Types needed to configure auth scheme resolution.")
            ClientRustModule.Config.endpoint -> strDoc("Types needed to configure endpoint resolution.")
            ClientRustModule.Config.retry -> strDoc("Retry configuration.")
            ClientRustModule.Config.timeout -> strDoc("Timeout configuration.")
            ClientRustModule.Config.http -> strDoc("HTTP request and response types.")
            ClientRustModule.Config.interceptors -> strDoc("Types needed to implement [`Intercept`](crate::config::Intercept).")
            ClientRustModule.Error -> strDoc("Common errors and error handling utilities.")
            ClientRustModule.Operation -> strDoc("All operations that this crate can perform.")
            ClientRustModule.Meta -> strDoc("Information about this crate.")
            ClientRustModule.Input -> PANIC("this module shouldn't exist in the new scheme")
            ClientRustModule.Output -> PANIC("this module shouldn't exist in the new scheme")
            ClientRustModule.primitives -> strDoc("Primitives such as `Blob` or `DateTime` used by other types.")
            ClientRustModule.Primitives.EventStream -> strDoc("Event stream related primitives such as `Message` or `Header`.")
            ClientRustModule.types -> strDoc("Data structures used by operation inputs/outputs.")
            ClientRustModule.Types.Error -> strDoc("Error types that $serviceName can respond with.")
            ClientRustModule.waiters -> strDoc("Supporting types for waiters.\n\nNote: to use waiters, import the [`Waiters`](crate::client::Waiters) trait, which adds methods prefixed with `wait_until` to the client.")
            else -> TODO("Document this module: $module")
        }
    }

    private fun clientModuleDoc(): Writable =
        writable {
            val genericClientConstructionDocs = FluentClientDocs.clientConstructionDocs(codegenContext)
            val writeClientConstructionDocs =
                codegenContext.rootDecorator
                    .clientConstructionDocs(codegenContext, genericClientConstructionDocs)

            writeClientConstructionDocs(this)
            FluentClientDocs.clientUsageDocs(codegenContext)(this)
            FluentClientDocs.waiterDocs(codegenContext)(this)
        }

    private fun customizeModuleDoc(): Writable =
        writable {
            val model = codegenContext.model
            val operations = TopDownIndex.of(model).getContainedOperations(codegenContext.serviceShape)
            docs("Operation customization and supporting types.\n")
            if (operations.isNotEmpty()) {
                val opFnName =
                    FluentClientGenerator.clientOperationFnName(
                        operations.minOf { it.id }
                            .let { model.expectShape(it, OperationShape::class.java) },
                        codegenContext.symbolProvider,
                    )
                val moduleUseName = codegenContext.moduleUseName()
                docsTemplate(
                    """
                    The underlying HTTP requests made during an operation can be customized
                    by calling the `customize()` method on the builder returned from a client
                    operation call. For example, this can be used to add an additional HTTP header:

                    ```ignore
                    ## async fn wrapper() -> #{Result}<(), $moduleUseName::Error> {
                    ## let client: $moduleUseName::Client = unimplemented!();
                    use #{http}::header::{HeaderName, HeaderValue};

                    let result = client.$opFnName()
                        .customize()
                        .mutate_request(|req| {
                            // Add `x-example-header` with value
                            req.headers_mut()
                                .insert(
                                    HeaderName::from_static("x-example-header"),
                                    HeaderValue::from_static("1"),
                                );
                        })
                        .send()
                        .await;
                    ## }
                    ```
                    """.trimIndent(),
                    *RuntimeType.preludeScope,
                    "http" to CargoDependency.Http1x.toDevDependency().toType(),
                )
            }
        }
}

object ClientModuleProvider : ModuleProvider {
    override fun moduleForShape(
        context: ModuleProviderContext,
        shape: Shape,
    ): RustModule.LeafModule {
        fun shouldNotBeRendered(): Nothing = PANIC("Shape ${shape.id} should not be rendered in any module")
        return when (shape) {
            is OperationShape -> perOperationModule(context, shape)
            is StructureShape ->
                when {
                    shape.hasTrait<ErrorTrait>() -> ClientRustModule.Types.Error
                    shape.hasTrait<SyntheticInputTrait>() -> perOperationModule(context, shape)
                    shape.hasTrait<SyntheticOutputTrait>() -> perOperationModule(context, shape)
                    else -> ClientRustModule.types
                }

            is UnionShape, is EnumShape -> ClientRustModule.types
            is StringShape -> {
                if (shape.hasTrait<EnumTrait>()) {
                    ClientRustModule.types
                } else {
                    shouldNotBeRendered()
                }
            }

            else -> shouldNotBeRendered()
        }
    }

    override fun moduleForOperationError(
        context: ModuleProviderContext,
        operation: OperationShape,
    ): RustModule.LeafModule = perOperationModule(context, operation)

    override fun moduleForEventStreamError(
        context: ModuleProviderContext,
        eventStream: UnionShape,
    ): RustModule.LeafModule = ClientRustModule.Types.Error

    override fun moduleForBuilder(
        context: ModuleProviderContext,
        shape: Shape,
        symbol: Symbol,
    ): RustModule.LeafModule =
        RustModule.public("builders", parent = symbol.module(), documentationOverride = "Builders")

    private fun Shape.findOperation(model: Model): OperationShape {
        val inputTrait = getTrait<SyntheticInputTrait>()
        val outputTrait = getTrait<SyntheticOutputTrait>()
        return when {
            this is OperationShape -> this
            inputTrait != null -> model.expectShape(inputTrait.operation, OperationShape::class.java)
            outputTrait != null -> model.expectShape(outputTrait.operation, OperationShape::class.java)
            else -> UNREACHABLE("this is only called with compatible shapes")
        }
    }

    private fun perOperationModule(
        context: ModuleProviderContext,
        shape: Shape,
    ): RustModule.LeafModule {
        val operationShape = shape.findOperation(context.model)
        val contextName = operationShape.contextName(context.serviceShape)
        val operationModuleName =
            RustReservedWords.escapeIfNeeded(contextName.toSnakeCase(), EscapeFor.ModuleName)
        return RustModule.public(
            operationModuleName,
            parent = ClientRustModule.Operation,
            documentationOverride = "Types for the `$contextName` operation.",
            // TODO(https://github.com/tokio-rs/tokio/issues/5683): Uncomment the NoImplicitPrelude attribute once this Tokio issue is resolved
            // // Disable the Rust prelude since every prelude type should be referenced with its
            // // fully qualified name to avoid name collisions with the generated operation shapes.
            // additionalAttributes = listOf(Attribute.NoImplicitPrelude)
        )
    }
}
