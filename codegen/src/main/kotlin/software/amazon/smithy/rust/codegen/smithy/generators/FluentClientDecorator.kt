/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asOptional
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class FluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext): Boolean =
        codegenContext.symbolProvider.config().codegenConfig.includeFluentClient

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        if (!applies(codegenContext)) {
            return
        }

        val module = RustMetadata(public = true)
        rustCrate.withModule(
            RustModule(
                "client",
                module,
                documentation = "Client and fluent builders for calling the service."
            )
        ) { writer ->
            FluentClientGenerator(
                codegenContext,
                customizations = listOf(GenericFluentClient(codegenContext))
            ).render(writer)
        }
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("aws-smithy-client/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("aws-smithy-client/native-tls")))
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        if (!applies(codegenContext)) {
            return baseCustomizations
        }

        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    rust("pub use client::{Client, Builder};")
                }
                else -> emptySection
            }
        }
    }
}

sealed class FluentClientSection(name: String) : Section(name) {
    /** Write custom code into an operation fluent builder's impl block */
    data class FluentBuilderImpl(
        val operationShape: OperationShape,
        val operationErrorType: RuntimeType
    ) : FluentClientSection("FluentBuilderImpl")

    /** Write custom code into the docs */
    data class FluentClientDocs(val serviceShape: ServiceShape) : FluentClientSection("FluentClientDocs")
}

abstract class FluentClientCustomization : NamedSectionGenerator<FluentClientSection>()

data class ClientGenerics(
    val connectorDefault: RuntimeType?,
    val middlewareDefault: RuntimeType?,
    val retryDefault: RuntimeType?,
    val client: RuntimeType
) {
    /** Declaration with defaults set */
    val decl = writable {
        rustTemplate(
            "<C #{c:W}, M#{m:W}, R#{r:W}>",
            "c" to defaultType(connectorDefault),
            "m" to defaultType(middlewareDefault),
            "r" to defaultType(retryDefault)
        )
    }

    /** Instantiation */
    val inst: String = "<C, M, R>"

    /** Trait bounds */
    val bounds = writable {
        rustTemplate(
            """
            C: #{client}::bounds::SmithyConnector,
            M: #{client}::bounds::SmithyMiddleware<C>,
            R: #{client}::retry::NewRequestPolicy,
            """,
            "client" to client
        )
    }

    private fun defaultType(default: RuntimeType?) = writable {
        default?.also { rust("= #T", default) }
    }
}

class GenericFluentClient(codegenContext: CodegenContext) : FluentClientCustomization() {
    private val moduleUseName = codegenContext.moduleUseName()
    private val clientDep = CargoDependency.SmithyClient(codegenContext.runtimeConfig)
    private val codegenScope = arrayOf("client" to clientDep.asType())
    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs -> writable {
                val humanName = section.serviceShape.id.name
                rust(
                    """
                    /// An ergonomic service client for `$humanName`.
                    ///
                    /// This client allows ergonomic access to a `$humanName`-shaped service.
                    /// Each method corresponds to an endpoint defined in the service's Smithy model,
                    /// and the request and response shapes are auto-generated from that same model.
                    /// """
                )
                rustTemplate(
                    """
                    /// ## Constructing a Client
                    ///
                    /// To construct a client, you need a few different things:
                    ///
                    /// - A [`Config`](crate::Config) that specifies additional configuration
                    ///   required by the service.
                    /// - A connector (`C`) that specifies how HTTP requests are translated
                    ///   into HTTP responses. This will typically be an HTTP client (like
                    ///   `hyper`), though you can also substitute in your own, like a mock
                    ///   mock connector for testing.
                    /// - A "middleware" (`M`) that modifies requests prior to them being
                    ///   sent to the request. Most commonly, middleware will decide what
                    ///   endpoint the requests should be sent to, as well as perform
                    ///   authentication and authorization of requests (such as SigV4).
                    ///   You can also have middleware that performs request/response
                    ///   tracing, throttling, or other middleware-like tasks.
                    /// - A retry policy (`R`) that dictates the behavior for requests that
                    ///   fail and should (potentially) be retried. The default type is
                    ///   generally what you want, as it implements a well-vetted retry
                    ///   policy implemented in [`retry::Standard`](crate::retry::Standard).
                    ///
                    /// To construct a client, you will generally want to call
                    /// [`Client::with_config`], which takes a [`#{client}::Client`] (a
                    /// Smithy client that isn't specialized to a particular service),
                    /// and a [`Config`](crate::Config). Both of these are constructed using
                    /// the [builder pattern] where you first construct a `Builder` type,
                    /// then configure it with the necessary parameters, and then call
                    /// `build` to construct the finalized output type. The
                    /// [`#{client}::Client`] builder is re-exported in this crate as
                    /// [`Builder`] for convenience.
                    ///
                    /// In _most_ circumstances, you will want to use the following pattern
                    /// to construct a client:
                    ///
                    /// ```
                    /// use $moduleUseName::{Builder, Client, Config};
                    /// let raw_client =
                    ///     Builder::dyn_https()
                    /// ##     /*
                    ///       .middleware(/* discussed below */)
                    /// ##     */
                    /// ##     .middleware_fn(|r| r)
                    ///       .build();
                    /// let config = Config::builder().build();
                    /// let client = Client::with_config(raw_client, config);
                    /// ```
                    ///
                    /// For the middleware, you'll want to use whatever matches the
                    /// routing, authentication and authorization required by the target
                    /// service. For example, for the standard AWS SDK which uses
                    /// [SigV4-signed requests], the middleware looks like this:
                    ///
                    // Ignored as otherwise we'd need to pull in all these dev-dependencies.
                    /// ```rust,ignore
                    /// use aws_endpoint::AwsEndpointStage;
                    /// use aws_http::user_agent::UserAgentStage;
                    /// use aws_sig_auth::middleware::SigV4SigningStage;
                    /// use aws_sig_auth::signer::SigV4Signer;
                    /// use aws_smithy_http_tower::map_request::MapRequestLayer;
                    /// use tower::layer::util::Stack;
                    /// use tower::ServiceBuilder;
                    ///
                    /// type AwsMiddlewareStack =
                    ///     Stack<MapRequestLayer<SigV4SigningStage>,
                    ///         Stack<MapRequestLayer<UserAgentStage>,
                    ///             MapRequestLayer<AwsEndpointStage>>>,
                    ///
                    /// ##[derive(Debug, Default)]
                    /// pub struct AwsMiddleware;
                    /// impl<S> tower::Layer<S> for AwsMiddleware {
                    ///     type Service = <AwsMiddlewareStack as tower::Layer<S>>::Service;
                    ///
                    ///     fn layer(&self, inner: S) -> Self::Service {
                    ///         let signer = MapRequestLayer::for_mapper(SigV4SigningStage::new(SigV4Signer::new())); _signer: MapRequestLaye
                    ///         let endpoint_resolver = MapRequestLayer::for_mapper(AwsEndpointStage); _endpoint_resolver: MapRequestLayer<Aw
                    ///         let user_agent = MapRequestLayer::for_mapper(UserAgentStage::new()); _user_agent: MapRequestLayer<UserAgentSt
                    ///         // These layers can be considered as occurring in order, that is:
                    ///         // 1. Resolve an endpoint
                    ///         // 2. Add a user agent
                    ///         // 3. Sign
                    ///         // (4. Dispatch over the wire)
                    ///         ServiceBuilder::new() _ServiceBuilder<Identity>
                    ///             .layer(endpoint_resolver) _ServiceBuilder<Stack<MapRequestLayer<_>, _>>
                    ///             .layer(user_agent) _ServiceBuilder<Stack<MapRequestLayer<_>, _>>
                    ///             .layer(signer) _ServiceBuilder<Stack<MapRequestLayer<_>, _>>
                    ///             .service(inner)
                    ///     }
                    /// }
                    /// ```
                    ///""",
                    *codegenScope
                )
                rust(
                    """
                    /// ## Using a Client
                    ///
                    /// Once you have a client set up, you can access the service's endpoints
                    /// by calling the appropriate method on [`Client`]. Each such method
                    /// returns a request builder for that endpoint, with methods for setting
                    /// the various fields of the request. Once your request is complete, use
                    /// the `send` method to send the request. `send` returns a future, which
                    /// you then have to `.await` to get the service's response.
                    ///
                    /// [builder pattern]: https://rust-lang.github.io/api-guidelines/type-safety.html##c-builder
                    /// [SigV4-signed requests]: https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html"""
                )
            }
            else -> emptySection
        }
    }
}

class FluentClientGenerator(
    codegenContext: CodegenContext,
    private val generics: ClientGenerics = ClientGenerics(
        connectorDefault = null,
        middlewareDefault = null,
        retryDefault = CargoDependency.SmithyClient(codegenContext.runtimeConfig).asType().member("retry::Standard"),
        client = CargoDependency.SmithyClient(codegenContext.runtimeConfig).asType()
    ),
    private val customizations: List<FluentClientCustomization> = emptyList(),
) {
    companion object {
        fun clientOperationFnName(operationShape: OperationShape, symbolProvider: RustSymbolProvider): String =
            RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operationShape).name.toSnakeCase())
    }

    private val serviceShape = codegenContext.serviceShape
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(serviceShape).sortedBy { it.id }
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val clientDep = CargoDependency.SmithyClient(codegenContext.runtimeConfig)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val core = FluentClientCore(model)

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[derive(Debug)]
            pub(crate) struct Handle#{generics_decl:W} {
                client: #{client}::Client${generics.inst},
                conf: crate::Config,
            }

            #{client_docs:W}
            ##[derive(std::fmt::Debug)]
            pub struct Client#{generics_decl:W} {
                handle: std::sync::Arc<Handle${generics.inst}>
            }

            impl${generics.inst} std::clone::Clone for Client${generics.inst} {
                fn clone(&self) -> Self {
                    Self { handle: self.handle.clone() }
                }
            }

            ##[doc(inline)]
            pub use #{client}::Builder;

            impl${generics.inst} From<#{client}::Client${generics.inst}> for Client${generics.inst} {
                fn from(client: #{client}::Client${generics.inst}) -> Self {
                    Self::with_config(client, crate::Config::builder().build())
                }
            }

            impl${generics.inst} Client${generics.inst} {
                /// Creates a client with the given service configuration.
                pub fn with_config(client: #{client}::Client${generics.inst}, conf: crate::Config) -> Self {
                    Self {
                        handle: std::sync::Arc::new(Handle {
                            client,
                            conf,
                        })
                    }
                }

                /// Returns the client's configuration.
                pub fn conf(&self) -> &crate::Config {
                    &self.handle.conf
                }
            }
            """,
            "generics_decl" to generics.decl,
            "client" to clientDep.asType(),
            "client_docs" to writable {
                customizations.forEach {
                    it.section(
                        FluentClientSection.FluentClientDocs(
                            serviceShape
                        )
                    )(this)
                }
            },
        )
        writer.rustBlockTemplate(
            "impl${generics.inst} Client${generics.inst} where #{bounds:W}",
            "client" to clientDep.asType(),
            "bounds" to generics.bounds
        ) {
            operations.forEach { operation ->
                val name = symbolProvider.toSymbol(operation).name
                rust(
                    """
                    /// Constructs a fluent builder for the `$name` operation.
                    ///
                    /// See [`$name`](crate::client::fluent_builders::$name) for more information about the
                    /// operation and its arguments.
                    pub fn ${
                    clientOperationFnName(
                        operation,
                        symbolProvider
                    )
                    }(&self) -> fluent_builders::$name${generics.inst} {
                        fluent_builders::$name::new(self.handle.clone())
                    }
                    """
                )
            }
        }
        writer.withModule("fluent_builders") {
            docs(
                """
                Utilities to ergonomically construct a request to the service.

                Fluent builders are created through the [`Client`](crate::client::Client) by calling
                one if its operation methods. After parameters are set using the builder methods,
                the `send` method can be called to initiate the request.
                """,
                newlinePrefix = "//! "
            )
            operations.forEach { operation ->
                val operationSymbol = symbolProvider.toSymbol(operation)
                val input = operation.inputShape(model)
                val members: List<MemberShape> = input.allMembers.values.toList()
                val baseDerives = symbolProvider.toSymbol(input).expectRustMetadata().derives
                val derives = baseDerives.derives.intersect(setOf(RuntimeType.Clone)) + RuntimeType.Debug
                rust(
                    """
                    /// Fluent builder constructing a request to `${operationSymbol.name}`.
                    ///
                    """
                )
                documentShape(operation, model, autoSuppressMissingDocs = false)
                baseDerives.copy(derives = derives).render(this)
                rustTemplate(
                    """
                    pub struct ${operationSymbol.name}#{generics:W} {
                        handle: std::sync::Arc<super::Handle${generics.inst}>,
                        inner: #{Inner}
                    }
                    """,
                    "Inner" to input.builderSymbol(symbolProvider),
                    "client" to clientDep.asType(),
                    "generics" to generics.decl,
                    "operation" to operationSymbol
                )

                rustBlockTemplate(
                    "impl${generics.inst} ${operationSymbol.name}${generics.inst} where #{bounds:W}",
                    "client" to clientDep.asType(),
                    "bounds" to generics.bounds
                ) {
                    rustTemplate(
                        """
                        /// Creates a new `${operationSymbol.name}`.
                        pub(crate) fn new(handle: std::sync::Arc<super::Handle${generics.inst}>) -> Self {
                            Self { handle, inner: Default::default() }
                        }

                        /// Sends the request and returns the response.
                        ///
                        /// If an error occurs, an `SdkError` will be returned with additional details that
                        /// can be matched against.
                        ///
                        /// By default, any retryable failures will be retried twice. Retry behavior
                        /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
                        /// set when configuring the client.
                        pub async fn send(self) -> std::result::Result<#{ok}, #{sdk_err}<#{operation_err}>>
                        where
                            R::Policy: #{client}::bounds::SmithyRetryPolicy<#{input}OperationOutputAlias,
                            #{ok},
                            #{operation_err},
                            #{input}OperationRetryAlias>,
                        {
                            let input = self.inner.build().map_err(|err|#{sdk_err}::ConstructionFailure(err.into()))?;
                            let op = input.make_operation(&self.handle.conf)
                                .await
                                .map_err(|err|#{sdk_err}::ConstructionFailure(err.into()))?;
                            self.handle.client.call(op).await
                        }
                        """,
                        "input" to symbolProvider.toSymbol(operation.inputShape(model)),
                        "ok" to symbolProvider.toSymbol(operation.outputShape(model)),
                        "operation_err" to operation.errorSymbol(symbolProvider),
                        "sdk_err" to CargoDependency.SmithyHttp(runtimeConfig).asType()
                            .copy(name = "result::SdkError"),
                        "client" to clientDep.asType(),
                    )
                    writeCustomizations(
                        customizations,
                        FluentClientSection.FluentBuilderImpl(
                            operation,
                            operation.errorSymbol(symbolProvider)
                        )
                    )
                    members.forEach { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        // All fields in the builder are optional
                        val memberSymbol = symbolProvider.toSymbol(member)
                        val outerType = memberSymbol.rustType()
                        when (val coreType = outerType.stripOuter<RustType.Option>()) {
                            is RustType.Vec -> with(core) { renderVecHelper(member, memberName, coreType) }
                            is RustType.HashMap -> with(core) { renderMapHelper(member, memberName, coreType) }
                            else -> {
                                val signature = when (coreType) {
                                    is RustType.String,
                                    is RustType.Box -> "(mut self, inp: impl Into<${coreType.render(true)}>) -> Self"
                                    else -> "(mut self, inp: ${coreType.render(true)}) -> Self"
                                }
                                documentShape(member, model)
                                rustBlock("pub fn $memberName$signature") {
                                    write("self.inner = self.inner.$memberName(inp);")
                                    write("self")
                                }
                            }
                        }
                        // pure setter
                        val inputType = outerType.asOptional()
                        documentShape(member, model)
                        rustBlock("pub fn ${member.setterName()}(mut self, input: ${inputType.render(true)}) -> Self") {
                            rust(
                                """
                                self.inner = self.inner.${member.setterName()}(input);
                                self
                                """
                            )
                        }
                    }
                }
            }
        }
    }
}
