/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault

/**
 * Generate a service-specific auth scheme resolver struct and a service-specific trait for auth scheme resolution.
 * ```rust
 * pub struct DefaultAuthSchemeResolver {
 *     service_defaults: Vec<AuthSchemeOption>,
 *     operation_overrides: HashMap<&'static str, Vec<AuthSchemeOption>>,
 * }
 *
 * impl crate::config::auth::ResolveEndpoint for DefaultAuthSchemeResolver {
 *      fn resolve_auth_scheme<'a>(
 *          &'a self,
 *          params: &'a crate::config::auth::Params,
 *          cfg: &'a ConfigBag,
 *          runtime_components: &'a RuntimeComponents,
 *      ) -> AuthSchemeOptionsFuture<'a> {
 *          // --snip--
 *      }
 * }
 * ```
 */
class AuthSchemeResolverGenerator(
    private val codegenContext: ClientCodegenContext,
    private val customizations: List<AuthCustomization>,
) {
    private val authIndex = AuthIndex(codegenContext)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val authSchemeParamsGenerator = AuthSchemeParamsGenerator(codegenContext)
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthSchemeOption" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemeOption"),
            "AuthSchemeOptionResolverParams" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::AuthSchemeOptionResolverParams"),
            "AuthSchemeOptionsFuture" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::auth::AuthSchemeOptionsFuture"),
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "Debug" to RuntimeType.Debug,
            "HashMap" to RuntimeType.HashMap,
            "Params" to authSchemeParamsGenerator.paramsStruct(),
            "ResolveAuthSchemeOptions" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::auth::ResolveAuthSchemeOptions"),
            "RuntimeComponents" to RuntimeType.runtimeComponents(runtimeConfig),
            "ServiceSpecificResolveAuthScheme" to serviceSpecificResolveAuthSchemeTrait(),
            "SharedAuthSchemeOptionResolver" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::auth::SharedAuthSchemeOptionResolver"),
        )

    /**
     * Return [RuntimeType] for the service-specific default auth scheme resolver
     */
    fun defaultAuthSchemeResolver(): RuntimeType {
        return RuntimeType.forInlineFun("DefaultAuthSchemeResolver", ClientRustModule.Config.auth) {
            rustTemplate(
                """
                /// The default auth scheme resolver
                ##[derive(Debug)]
                ##[allow(dead_code)]
                pub struct DefaultAuthSchemeResolver {
                    service_defaults: Vec<#{AuthSchemeOption}>,
                    operation_overrides: #{HashMap}<&'static str, Vec<#{AuthSchemeOption}>>,
                }

                // TODO(https://github.com/smithy-lang/smithy-rs/issues/4177): Remove `allow(...)` once the issue is addressed.
                // When generating code for tests (e.g., `codegen-client-test`), this manual implementation
                // of the `Default` trait may appear as if it could be derived automatically.
                // However, that is not the case in production.
                ##[allow(clippy::derivable_impls)]
                impl Default for DefaultAuthSchemeResolver {
                    fn default() -> Self {
                        Self {
                            service_defaults: vec![#{service_defaults:W}],
                            operation_overrides: #{operation_overrides:W},
                        }
                    }
                }

                impl #{ServiceSpecificResolveAuthScheme} for DefaultAuthSchemeResolver {
                    fn resolve_auth_scheme<'a>(
                        &'a self,
                        params: &'a #{Params},
                        _cfg: &'a #{ConfigBag},
                        _runtime_components: &'a #{RuntimeComponents},
                    ) -> #{AuthSchemeOptionsFuture}<'a> {
                        let operation_name = params.operation_name();

                        let modeled_auth_options = match self.operation_overrides.get(operation_name) {
                            Some(overrides) => overrides,
                            None => &self.service_defaults,
                        };

                        let _fut = #{AuthSchemeOptionsFuture}::ready(Ok(modeled_auth_options.clone()));

                        #{additional_impl:W}

                        _fut
                    }
                }
                """,
                *codegenScope,
                "service_defaults" to
                    authIndex.effectiveAuthOptionsForService().map {
                        it.render(codegenContext)
                    }.join(", "),
                "operation_overrides" to renderOperationAuthOptionOverrides(),
                "additional_impl" to
                    writable {
                        writeCustomizations(
                            customizations,
                            AuthSection.DefaultResolverAdditionalImpl,
                        )
                    },
            )
        }
    }

    private fun renderOperationAuthOptionOverrides() =
        writable {
            val operationsWithOverrides = authIndex.operationsWithOverrides()
            if (operationsWithOverrides.isEmpty()) {
                rustTemplate("#{HashMap}::new()", *codegenScope)
            } else {
                withBlock("[", "]") {
                    operationsWithOverrides.forEach { op ->
                        val operationAuthSchemes =
                            authIndex.effectiveAuthOptionsForOperation(op)
                        if (operationAuthSchemes.isNotEmpty()) {
                            rustTemplate(
                                "(${op.id.name.dq()}, vec![#{auth_options:W}])",
                                "auth_options" to
                                    operationAuthSchemes.map {
                                        it.render(codegenContext, op)
                                    }.join(", "),
                            )
                            rust(", ")
                        }
                    }
                }
                rust(".into()")
            }
        }

    /**
     * Return [RuntimeType] representing the per-service trait definition for auth scheme resolution
     */
    fun serviceSpecificResolveAuthSchemeTrait(): RuntimeType {
        return RuntimeType.forInlineFun("ResolveAuthScheme", ClientRustModule.Config.auth) {
            rustTemplate(
                """
                /// Auth scheme resolver trait specific to ${codegenContext.serviceShape.serviceNameOrDefault("this service")}
                pub trait ResolveAuthScheme: #{Send} + #{Sync} + #{Debug} {
                    /// Resolve a priority list of auth scheme options with the given parameters
                    fn resolve_auth_scheme<'a>(
                        &'a self,
                        params: &'a #{Params},
                        cfg: &'a #{ConfigBag},
                        runtime_components: &'a #{RuntimeComponents},
                    ) -> #{AuthSchemeOptionsFuture}<'a>;

                    /// Convert this service-specific resolver into a `SharedAuthSchemeOptionResolver`
                    fn into_shared_resolver(self) -> #{SharedAuthSchemeOptionResolver}
                    where
                        Self: #{Sized} + 'static,
                    {
                        #{SharedAuthSchemeOptionResolver}::new(DowncastParams(self))
                    }
                }

                ##[derive(Debug)]
                struct DowncastParams<T>(T);
                impl<T> #{ResolveAuthSchemeOptions} for DowncastParams<T>
                where
                    T: ResolveAuthScheme,
                {
                    fn resolve_auth_scheme_options_v2<'a>(
                        &'a self,
                        params: &'a #{AuthSchemeOptionResolverParams},
                        cfg: &'a #{ConfigBag},
                        runtime_components: &'a #{RuntimeComponents},
                    ) -> #{AuthSchemeOptionsFuture}<'a> {
                        match params.get::<#{Params}>() {
                            #{Some}(concrete_params) => self.0.resolve_auth_scheme(concrete_params, cfg, runtime_components),
                            #{None} => #{AuthSchemeOptionsFuture}::ready(#{Err}("params of expected type was not present".into())),
                        }
                    }
                }
                """,
                *codegenScope,
            )
        }
    }
}
