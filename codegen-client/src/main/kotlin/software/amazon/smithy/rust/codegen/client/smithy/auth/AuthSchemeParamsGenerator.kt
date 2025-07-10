/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Generate types for a service-specific auth scheme parameters
 */
class AuthSchemeParamsGenerator(codegenContext: ClientCodegenContext) {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthSchemeOption" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemeOption"),
            "Cow" to RuntimeType.Cow,
            "Params" to paramsStruct(),
        )

    /**
     * Return [RuntimeType] for a service-specific auth scheme params struct
     */
    fun paramsStruct(): RuntimeType =
        RuntimeType.forInlineFun("Params", ClientRustModule.Config.auth) {
            generateAuthSchemeParamsStruct()
        }

    private fun paramsBuilder(): RuntimeType =
        RuntimeType.forInlineFun("ParamsBuilder", ClientRustModule.Config.auth) {
            generateAuthSchemeParamsBuilder()
        }

    private fun paramsBuildError(): RuntimeType =
        RuntimeType.forInlineFun("BuildError", ClientRustModule.Config.auth) {
            rustTemplate(
                """
                /// An error that occurred while constructing `config::auth::Params`
                ##[derive(Debug)]
                pub struct BuildError {
                    field: #{Cow}<'static, str>
                }

                impl BuildError {
                    fn missing(field: &'static str) -> Self {
                        Self { field: field.into() }
                    }
                }

                impl std::fmt::Display for BuildError {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "a required field was missing: `{}`", self.field)
                    }
                }

                impl std::error::Error for BuildError { }
                """,
                *codegenScope,
            )
        }

    private fun RustWriter.generateAuthSchemeParamsStruct() {
        docs("Configuration parameters for resolving the correct auth scheme")
        Attribute(derive(RuntimeType.Debug, RuntimeType.PartialEq, RuntimeType.Clone)).render(this)
        rustBlock("pub struct Params") {
            rustTemplate("operation_name: #{Cow}<'static, str>", *codegenScope)
        }

        rustBlock("impl Params") {
            rustTemplate(
                """
                /// Create a builder for [`Params`]
                pub fn builder() -> #{ParamsBuilder} {
                    #{ParamsBuilder}::default()
                }

                /// Return the operation name for [`Params`]
                pub fn operation_name(&self) -> &str {
                    self.operation_name.as_ref()
                }
                """,
                "ParamsBuilder" to paramsBuilder(),
            )
        }
    }

    private fun RustWriter.generateAuthSchemeParamsBuilder() {
        Attribute(derive(RuntimeType.Clone, RuntimeType.Debug, RuntimeType.Default, RuntimeType.PartialEq)).render(this)
        docs("Builder for [`Params`]")
        rustBlock("pub struct ParamsBuilder") {
            rustTemplate(
                """
                operation_name: #{Option}<#{Cow}<'static, str>>,
                """,
                *codegenScope,
            )
        }

        rustBlock("impl ParamsBuilder") {
            rustTemplate(
                """
                /// Set the operation name for the builder
                pub fn operation_name(self, operation_name: impl Into<#{Cow}<'static, str>>) -> Self {
                    self.set_operation_name(#{Some}(operation_name.into()))
                }

                /// Set the operation name for the builder
                pub fn set_operation_name(mut self, operation_name: #{Option}<#{Cow}<'static, str>>) -> Self {
                    self.operation_name = operation_name;
                    self
                }
                """,
                *codegenScope,
            )
            docs(
                """Consume this builder, create [`Params`]."

                Return [`BuildError`] if any of the required fields are unset.
                """,
            )
            rustBlockTemplate(
                "pub fn build(self) -> #{Result}<#{Params}, #{BuildError}>",
                *preludeScope,
                "Params" to paramsStruct(),
                "BuildError" to paramsBuildError(),
            ) {
                rustTemplate(
                    """
                    #{Ok}(#{Params} {
                        operation_name: self.operation_name
                            .ok_or_else(||BuildError::missing("operation_name"))?
                    })
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
