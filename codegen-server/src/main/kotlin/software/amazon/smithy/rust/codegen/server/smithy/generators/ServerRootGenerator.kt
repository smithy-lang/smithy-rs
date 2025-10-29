/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Error as ErrorModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Input as InputModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Output as OutputModule

/**
 * ServerRootGenerator
 *
 * Generates all code within `lib.rs`, this includes:
 *  - Crate documentation
 *  - Re-exports
 */
open class ServerRootGenerator(
    val protocol: ServerProtocol,
    private val codegenContext: ServerCodegenContext,
    private val isConfigBuilderFallible: Boolean,
) {
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations =
        index.getContainedOperations(codegenContext.serviceShape).toSortedSet(
            compareBy {
                it.id
            },
        ).toList()
    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()

    fun documentation(writer: RustWriter) {
        val builderFieldNames =
            operations.associateWith {
                RustReservedWords.escapeIfNeeded(codegenContext.symbolProvider.toSymbol(it).name.toSnakeCase())
            }
                .toSortedMap(
                    compareBy { it.id },
                )
        val crateName = codegenContext.moduleUseName()
        val builderName = "${serviceName}Builder"
        val hasErrors = operations.any { it.errors.isNotEmpty() }
        val handlers: Writable =
            operations
                .map { operation ->
                    DocHandlerGenerator(codegenContext, operation, builderFieldNames[operation]!!, "//!").docSignature()
                }
                .join("//!\n")

        val unwrapConfigBuilder = if (isConfigBuilderFallible) ".expect(\"config failed to build\")" else ""

        writer.rustTemplate(
            """
            //! A fast and customizable Rust implementation of the $serviceName Smithy service.
            //!
            //! ## Using $serviceName
            //!
            //! The primary entrypoint is [`$serviceName`]: it satisfies the [`Service<http::Request, Response = http::Response>`](#{Tower}::Service)
            //! trait and therefore can be handed to a [`hyper` server](https://github.com/hyperium/hyper) via [`$serviceName::into_make_service`]
            //! or used in AWS Lambda
            ##![cfg_attr(
                feature = "aws-lambda",
                doc = " via [`LambdaHandler`](crate::server::routing::LambdaHandler).")]
            ##![cfg_attr(
                not(feature = "aws-lambda"),
                doc = " by enabling the `aws-lambda` feature flag and utilizing the `LambdaHandler`.")]
            //! The [`crate::${InputModule.name}`], ${if (!hasErrors) "and " else ""}[`crate::${OutputModule.name}`], ${if (hasErrors) "and [`crate::${ErrorModule.name}`]" else "" }
            //! modules provide the types used in each operation.
            //!
            ${if (codegenContext.isHttp1()) {
                """
                //! ###### Quick Start - Using `serve`
                //!
                //! The simplest way to run your service is using the [`server::serve`] function:
                //!
                //! ```rust,no_run
                //! ## use std::net::SocketAddr;
                //! ## async fn dummy() {
                //! use $crateName::{$serviceName, ${serviceName}Config};
                //! use #{Tokio}::net::TcpListener;
                //!
                //! ## let app = $serviceName::builder(
                //! ##     ${serviceName}Config::builder()
                //! ##         .build()$unwrapConfigBuilder
                //! ## ).build_unchecked();
                //! let listener = TcpListener::bind("127.0.0.1:6969").await.expect("failed to bind");
                //! $crateName::server::serve(listener, app.into_make_service()).await.expect("server error");
                //! ## }
                //! ```
                //!
                //! For graceful shutdown:
                //!
                //! ```rust,no_run
                //! ## use std::net::SocketAddr;
                //! ## async fn dummy() {
                //! use $crateName::{$serviceName, ${serviceName}Config};
                //! use #{Tokio}::net::TcpListener;
                //! use #{Tokio}::signal;
                //!
                //! ## let app = $serviceName::builder(
                //! ##     ${serviceName}Config::builder()
                //! ##         .build()$unwrapConfigBuilder
                //! ## ).build_unchecked();
                //! let listener = TcpListener::bind("127.0.0.1:6969").await.expect("failed to bind");
                //! $crateName::server::serve(listener, app.into_make_service())
                //!     .with_graceful_shutdown(async {
                //!         signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
                //!     })
                //!     .await
                //!     .expect("server error");
                //! ## }
                //! ```
                //!
                //! ###### Advanced - Using Hyper Directly
                //!
                //! For more control over the server (custom executors, HTTP/2 settings, etc.),
                //! you can use hyper-util directly:
                """.trimIndent()
            } else {
                ""
            }}
            //!
            ${if (!codegenContext.isHttp1()) "//! ###### Running on Hyper\n//!" else ""}
            //! ```rust,no_run
            //! ## use std::net::SocketAddr;
            //! ## async fn dummy() {
            //! use $crateName::{$serviceName, ${serviceName}Config};
            //!
            //! ## let app = $serviceName::builder(
            //! ##     ${serviceName}Config::builder()
            //! ##         .build()$unwrapConfigBuilder
            //! ## ).build_unchecked();
            ${if (codegenContext.isHttp1()) {
                """
                //! use #{HyperUtil}::rt::TokioIo;
                //! use #{HyperUtil}::rt::TokioExecutor;
                //! use #{Tokio}::net::TcpListener;
                //! use #{Tower}::Service;
                //!
                //! let bind: SocketAddr = "127.0.0.1:6969".parse()
                //!     .expect("unable to parse the server bind address and port");
                //! let listener = TcpListener::bind(bind).await.expect("failed to bind");
                //!
                //! loop {
                //!     let (stream, remote_addr) = listener.accept().await.expect("failed to accept connection");
                //!     let tower_service = app.clone();
                //!
                //!     #{Tokio}::task::spawn(async move {
                //!         let io = TokioIo::new(stream);
                //!         let hyper_service = #{Hyper}::service::service_fn(move |request| {
                //!             tower_service.clone().call(request)
                //!         });
                //!
                //!         if let Err(err) = #{HyperUtil}::server::conn::auto::Builder::new(TokioExecutor::new())
                //!             .serve_connection_with_upgrades(io, hyper_service)
                //!             .await
                //!         {
                //!             eprintln!("Error serving connection: {:?}", err);
                //!         }
                //!     });
                //! }
                """.trimIndent()
            } else {
                """
                //! let server = app.into_make_service();
                //! let bind: SocketAddr = "127.0.0.1:6969".parse()
                //!     .expect("unable to parse the server bind address and port");
                //! #{Hyper}::Server::bind(&bind).serve(server).await.unwrap();
                """.trimIndent()
            }}
            //! ## }
            //! ```
            //!
            //! ###### Running on Lambda
            //!
            //! ```rust,ignore
            //! use $crateName::server::routing::LambdaHandler;
            //! use $crateName::$serviceName;
            //!
            //! ## async fn dummy() {
            //! ## let app = $serviceName::builder(
            //! ##     ${serviceName}Config::builder()
            //! ##         .build()$unwrapConfigBuilder
            //! ## ).build_unchecked();
            //! let handler = LambdaHandler::new(app);
            //! lambda_http::run(handler).await.unwrap();
            //! ## }
            //! ```
            //!
            //! ## Building the $serviceName
            //!
            //! To construct [`$serviceName`] we use [`$builderName`] returned by [`$serviceName::builder`].
            //!
            //! #### Plugins
            //!
            //! The [`$serviceName::builder`] method, returning [`$builderName`],
            //! accepts a config object on which plugins can be registered.
            //! Plugins allow you to build middleware which is aware of the operation it is being applied to.
            //!
            //! ```rust,no_run
            //! ## use $crateName::server::plugin::IdentityPlugin as LoggingPlugin;
            //! ## use $crateName::server::plugin::IdentityPlugin as MetricsPlugin;
            ${if (codegenContext.isHttp1()) "//! use $crateName::server::body::BoxBody;" else "//! ## use #{Hyper}::Body;"}
            //! use $crateName::server::plugin::HttpPlugins;
            ${if (codegenContext.isHttp1()) "//! use $crateName::{$serviceName, ${serviceName}Config};" else "//! use $crateName::{$serviceName, ${serviceName}Config, $builderName};"}
            //!
            //! let http_plugins = HttpPlugins::new()
            //!         .push(LoggingPlugin)
            //!         .push(MetricsPlugin);
            //! let config = ${serviceName}Config::builder().http_plugin(http_plugins).build()$unwrapConfigBuilder;
            ${if (codegenContext.isHttp1()) "//! let _app = $serviceName::builder::<BoxBody, _, _, _>(config).build_unchecked();" else "//! let builder: $builderName<Body, _, _, _> = $serviceName::builder(config);"}
            //! ```
            //!
            //! Check out [`crate::server::plugin`] to learn more about plugins.
            //!
            //! #### Handlers
            //!
            //! [`$builderName`] provides a setter method for each operation in your Smithy model. The setter methods expect an async function as input, matching the signature for the corresponding operation in your Smithy model.
            //! We call these async functions **handlers**. This is where your application business logic lives.
            //!
            //! Every handler must take an `Input`, and optional [`extractor arguments`](crate::server::request), while returning:
            //!
            //! * A `Result<Output, Error>` if your operation has modeled errors, or
            //! * An `Output` otherwise.
            //!
            //! ```rust,no_run
            //! ## struct Input;
            //! ## struct Output;
            //! ## struct Error;
            //! async fn infallible_handler(input: Input) -> Output { todo!() }
            //!
            //! async fn fallible_handler(input: Input) -> Result<Output, Error> { todo!() }
            //! ```
            //!
            //! Handlers can accept up to 8 extractors:
            //!
            //! ```rust,no_run
            //! ## struct Input;
            //! ## struct Output;
            //! ## struct Error;
            //! ## struct State;
            //! ## use std::net::SocketAddr;
            //! use $crateName::server::request::{extension::Extension, connect_info::ConnectInfo};
            //!
            //! async fn handler_with_no_extensions(input: Input) -> Output {
            //!     todo!()
            //! }
            //!
            //! async fn handler_with_one_extractor(input: Input, ext: Extension<State>) -> Output {
            //!     todo!()
            //! }
            //!
            //! async fn handler_with_two_extractors(
            //!     input: Input,
            //!     ext0: Extension<State>,
            //!     ext1: ConnectInfo<SocketAddr>,
            //! ) -> Output {
            //!     todo!()
            //! }
            //! ```
            //!
            //! See the [`operation module`](crate::operation) for information on precisely what constitutes a handler.
            //!
            //! #### Build
            //!
            //! You can convert [`$builderName`] into [`$serviceName`] using either [`$builderName::build`] or [`$builderName::build_unchecked`].
            //!
            //! [`$builderName::build`] requires you to provide a handler for every single operation in your Smithy model. It will return an error if that is not the case.
            //!
            //! [`$builderName::build_unchecked`], instead, does not require exhaustiveness. The server will automatically return 500 Internal Server Error to all requests for operations that do not have a registered handler.
            //! [`$builderName::build_unchecked`] is particularly useful if you are deploying your Smithy service as a collection of Lambda functions, where each Lambda is only responsible for a subset of the operations in the Smithy service (or even a single one!).
            //!
            //! ## Example
            //!
            //! ```rust,no_run
            //! ## use std::net::SocketAddr;
            //! use $crateName::{$serviceName, ${serviceName}Config};
            //!
            //! ##[#{Tokio}::main]
            //! pub async fn main() {
            //!    let config = ${serviceName}Config::builder().build()$unwrapConfigBuilder;
            //!    let app = $serviceName::builder(config)
            ${builderFieldNames.values.joinToString("\n") { "//!        .$it($it)" }}
            //!        .build()
            //!        .expect("failed to build an instance of $serviceName");
            //!
            ${if (codegenContext.isHttp1()) {
                """
                //!    use #{HyperUtil}::rt::TokioIo;
                //!    use #{HyperUtil}::rt::TokioExecutor;
                //!    use #{Tokio}::net::TcpListener;
                //!    use #{Tower}::Service;
                //!
                //!    let bind: SocketAddr = "127.0.0.1:6969".parse()
                //!        .expect("unable to parse the server bind address and port");
                //!    let listener = TcpListener::bind(bind).await.expect("failed to bind");
                //!
                //!    loop {
                //!        let (stream, remote_addr) = listener.accept().await.expect("failed to accept connection");
                //!        let tower_service = app.clone();
                //!
                //!        #{Tokio}::task::spawn(async move {
                //!            let io = TokioIo::new(stream);
                //!            let hyper_service = #{Hyper}::service::service_fn(move |request| {
                //!                tower_service.clone().call(request)
                //!            });
                //!
                //!            if let Err(err) = #{HyperUtil}::server::conn::auto::Builder::new(TokioExecutor::new())
                //!                .serve_connection_with_upgrades(io, hyper_service)
                //!                .await
                //!            {
                //!                eprintln!("Error serving connection: {:?}", err);
                //!            }
                //!        });
                //!    }
                """.trimIndent()
            } else {
                """
                //!    let bind: SocketAddr = "127.0.0.1:6969".parse()
                //!        .expect("unable to parse the server bind address and port");
                //!    let server = #{Hyper}::Server::bind(&bind).serve(app.into_make_service());
                //!    ## let server = async { Ok::<_, ()>(()) };
                //!
                //!    // Run your service!
                //!    if let Err(err) = server.await {
                //!        eprintln!("server error: {:?}", err);
                //!    }
                """.trimIndent()
            }}
            //! }
            //!
            #{HandlerImports:W}
            //!
            #{Handlers:W}
            //!
            //! ```
            //!
            //! [`serve`]: https://docs.rs/hyper/0.14.16/hyper/server/struct.Builder.html##method.serve
            //! [`tower::make::MakeService`]: https://docs.rs/tower/latest/tower/make/trait.MakeService.html
            //! [HTTP binding traits]: https://smithy.io/2.0/spec/http-bindings.html
            //! [operations]: https://smithy.io/2.0/spec/service-types.html##operation
            //! [hyper server]: https://docs.rs/hyper/latest/hyper/server/index.html
            //! [Service]: https://docs.rs/tower-service/latest/tower_service/trait.Service.html
            """,
            "HandlerImports" to handlerImports(crateName, operations, commentToken = "//!"),
            "Handlers" to handlers,
            "ExampleHandler" to operations.take(1).map { operation -> DocHandlerGenerator(codegenContext, operation, builderFieldNames[operation]!!, "//!").docSignature() },
            "Hyper" to codegenContext.httpDependencies().hyperDevModule(),
            "HyperUtil" to RuntimeType("hyper_util", CargoDependency("hyper-util", CratesIo("0.1"), scope = DependencyScope.Dev, features = setOf("service"))),
            "Tokio" to ServerCargoDependency.TokioDev.toType(),
            "Tower" to ServerCargoDependency.Tower.toType(),
        )
    }

    /**
     * Render Service Specific code. Code will end up in different files via [useShapeWriter]. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render(rustWriter: RustWriter) {
        documentation(rustWriter)

        // Only export config builder error if fallible.
        val configErrorReExport =
            if (isConfigBuilderFallible) {
                "${serviceName}ConfigError,"
            } else {
                ""
            }
        rustWriter.rust(
            """
            pub use crate::service::{
                $serviceName,
                ${serviceName}Config,
                ${serviceName}ConfigBuilder,
                $configErrorReExport
                ${serviceName}Builder,
                MissingOperationsError
            };
            """,
        )
    }
}
