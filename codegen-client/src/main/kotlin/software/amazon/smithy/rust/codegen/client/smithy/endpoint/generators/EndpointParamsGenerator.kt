/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.evaluation.value.ArrayValue
import software.amazon.smithy.rulesengine.language.evaluation.value.BooleanValue
import software.amazon.smithy.rulesengine.language.evaluation.value.StringValue
import software.amazon.smithy.rulesengine.language.evaluation.value.Value
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.memberName
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.symbol
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.asDeref
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.isCopy
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

// internals contains the actual resolver function
fun endpointImplModule() = RustModule.private("internals", parent = ClientRustModule.Config.endpoint)

fun endpointTestsModule() =
    RustModule.new(
        "test",
        visibility = Visibility.PRIVATE,
        parent = ClientRustModule.Config.endpoint,
        inline = true,
        documentationOverride = "",
    ).cfgTest()

// stdlib is isolated because it contains code generated names of stdlib functions–we want to ensure we avoid clashing
val EndpointStdLib = RustModule.private("endpoint_lib")

/** Endpoint Parameters generator.
 *
 * This class generates the `Params` struct for an [EndpointRuleset]. The struct has `pub(crate)` fields, a `Builder`,
 * and an error type, `InvalidParams` that is created to handle when construction fails.
 *
 * The builder of this struct generates a fallible `build()` method because endpoint params MAY have required fields.
 * However, the external parts of this struct (the public accessors) will _always_ be optional to ensure a public
 * interface is maintained.
 *
 * The following snippet contains an example of what is generated (eliding the error):
 *  ```rust
 *  #[non_exhaustive]
 *  #[derive(std::clone::Clone, std::cmp::PartialEq, std::fmt::Debug)]
 *  /// Configuration parameters for resolving the correct endpoint
 *  pub struct Params {
 *      pub(crate) region: std::option::Option<std::string::String>,
 *  }
 *  impl Params {
 *      /// Create a builder for [`Params`]
 *      pub fn builder() -> crate::endpoint::ParamsBuilder {
 *          crate::endpoint::Builder::default()
 *      }
 *      /// Gets the value for region
 *      pub fn region(&self) -> std::option::Option<&str> {
 *          self.region.as_deref()
 *      }
 *  }
 *
 *  /// Builder for [`Params`]
 *  #[derive(std::default::Default, std::clone::Clone, std::cmp::PartialEq, std::fmt::Debug)]
 *  pub struct ParamsBuilder {
 *      region: std::option::Option<std::string::String>,
 *  }
 *  impl ParamsBuilder {
 *      /// Consume this builder, creating [`Params`].
 *      pub fn build(
 *          self,
 *      ) -> Result<crate::endpoint::Params, crate::endpoint::Error> {
 *          Ok(crate::endpoint::Params {
 *                  region: self.region,
 *          })
 *      }
 *
 *      /// Sets the value for region
 *      pub fn region(mut self, value: std::string::String) -> Self {
 *          self.region = Some(value);
 *          self
 *      }
 *
 *      /// Sets the value for region
 *      pub fn set_region(mut self, param: Option<impl Into<std::string::String>>) -> Self {
 *          self.region = param.map(|t| t.into());
 *          self
 *      }
 *  }
 *  ```
 */

class EndpointParamsGenerator(
    private val codegenContext: ClientCodegenContext,
    private val parameters: Parameters,
) {
    companion object {
        fun memberName(parameterName: String) = Identifier.of(parameterName).rustName()

        fun setterName(parameterName: String) = "set_${memberName(parameterName)}"

        fun getterName(parameterName: String) = "get_${memberName(parameterName)}"

        fun paramsError(): RuntimeType =
            RuntimeType.forInlineFun("InvalidParams", ClientRustModule.Config.endpoint) {
                rust(
                    """
                    /// An error that occurred during endpoint resolution
                    ##[derive(Debug)]
                    pub struct InvalidParams {
                        field: std::borrow::Cow<'static, str>,
                        kind: InvalidParamsErrorKind,
                    }

                    /// The kind of invalid parameter error
                    ##[derive(Debug)]
                    enum InvalidParamsErrorKind {
                        MissingField,
                        InvalidValue {
                            message: &'static str,
                        }
                    }

                    impl InvalidParams {
                        ##[allow(dead_code)]
                        fn missing(field: &'static str) -> Self {
                            Self { field: field.into(), kind: InvalidParamsErrorKind::MissingField }
                        }

                        ##[allow(dead_code)]
                        fn invalid_value(field: &'static str, message: &'static str) -> Self {
                            Self { field: field.into(), kind: InvalidParamsErrorKind::InvalidValue { message }}
                        }
                    }

                    impl std::fmt::Display for InvalidParams {
                        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            match self.kind {
                                InvalidParamsErrorKind::MissingField => write!(f, "a required field was missing: `{}`", self.field),
                                InvalidParamsErrorKind::InvalidValue { message} => write!(f, "invalid value for field: `{}` - {}", self.field, message),
                            }
                        }
                    }

                    impl std::error::Error for InvalidParams { }
                    """,
                )
            }
    }

    fun paramsStruct(): RuntimeType =
        RuntimeType.forInlineFun("Params", ClientRustModule.Config.endpoint) {
            generateEndpointsStruct(this)
        }

    internal fun paramsBuilder(): RuntimeType =
        RuntimeType.forInlineFun("ParamsBuilder", ClientRustModule.Config.endpoint) {
            generateEndpointParamsBuilder(this)
        }

    /**
     * Generates an endpoints struct based on the provided endpoint rules. The struct fields are `pub(crate)`
     * with optionality as indicated by the required status of the parameter.
     */
    private fun generateEndpointsStruct(writer: RustWriter) {
        // Ensure that fields can be added in the future
        Attribute.NonExhaustive.render(writer)
        // Automatically implement standard Rust functionality
        Attribute(derive(RuntimeType.Debug, RuntimeType.PartialEq, RuntimeType.Clone)).render(writer)
        // Generate the struct block:
        //    pub struct Params {
        //        ... members: pub(crate) field
        //    }
        writer.docs("Configuration parameters for resolving the correct endpoint")
        writer.rustBlock("pub struct Params") {
            parameters.toList().forEach { parameter ->
                // Render documentation for each parameter
                parameter.documentation.orNull()?.also { docs(it) }
                rust("pub(crate) ${parameter.memberName()}: #T,", parameter.symbol())
            }
        }

        // Generate the impl block for the struct
        writer.rustBlock("impl Params") {
            rustTemplate(
                """
                /// Create a builder for [`Params`]
                pub fn builder() -> #{Builder} {
                    #{Builder}::default()
                }
                """,
                "Builder" to paramsBuilder(),
            )
            parameters.toList().forEach { parameter ->
                val name = parameter.memberName()
                val type = parameter.symbol()

                (parameter.documentation.orNull() ?: "Gets the value for `$name`").also { docs(it) }
                rustTemplate(
                    """
                    pub fn ${parameter.memberName()}(&self) -> #{paramType} {
                        #{param:W}
                    }

                    """,
                    "paramType" to type.makeOptional().mapRustType { t -> t.asDeref() },
                    "param" to
                        writable {
                            when {
                                type.isOptional() && type.rustType().isCopy() -> rust("self.$name")
                                type.isOptional() -> rust("self.$name.as_deref()")
                                type.rustType().isCopy() -> rust("Some(self.$name)")
                                else -> rust("Some(&self.$name)")
                            }
                        },
                )
            }
        }
    }

    private fun value(value: Value): String {
        return when (value) {
            is StringValue -> value.value.dq() + ".to_string()"
            is BooleanValue -> value.value.toString()
            is ArrayValue -> {
                val elements = value.values.joinToString(", ") { value(it) }
                "vec![$elements]"
            }

            else -> TODO("unexpected type: $value")
        }
    }

    private fun generateEndpointParamsBuilder(rustWriter: RustWriter) {
        rustWriter.docs("Builder for [`Params`]")
        Attribute(derive(RuntimeType.Debug, RuntimeType.Default, RuntimeType.PartialEq, RuntimeType.Clone)).render(
            rustWriter,
        )
        rustWriter.rustBlock("pub struct ParamsBuilder") {
            parameters.toList().forEach { parameter ->
                val name = parameter.memberName()
                val type = parameter.symbol().makeOptional()
                rust("$name: #T,", type)
            }
        }

        rustWriter.rustBlock("impl ParamsBuilder") {
            docs("Consume this builder, creating [`Params`].")
            rustBlockTemplate(
                "pub fn build(self) -> #{Result}<#{Params}, #{ParamsError}>",
                *preludeScope,
                "Params" to paramsStruct(),
                "ParamsError" to paramsError(),
            ) {
                // additional validation for endpoint parameters during construction
                parameters.toList().forEach { parameter ->
                    val validators =
                        codegenContext.rootDecorator.endpointCustomizations(codegenContext)
                            .mapNotNull { it.endpointParamsBuilderValidator(codegenContext, parameter) }

                    validators.forEach { validator ->
                        rust("#W;", validator)
                    }
                }

                val params =
                    writable {
                        Attribute.AllowClippyUnnecessaryLazyEvaluations.render(this)
                        rustBlockTemplate("#{Params}", "Params" to paramsStruct()) {
                            parameters.toList().forEach { parameter ->
                                rust("${parameter.memberName()}: self.${parameter.memberName()}")
                                parameter.default.orNull()
                                    ?.also { default -> rust(".or_else(||Some(${value(default)}))") }
                                if (parameter.isRequired) {
                                    rustTemplate(
                                        ".ok_or_else(||#{Error}::missing(${parameter.memberName().dq()}))?",
                                        "Error" to paramsError(),
                                    )
                                }
                                rust(",")
                            }
                        }
                    }
                rust("Ok(#W)", params)
            }
            parameters.toList().forEach { parameter ->
                val name = parameter.memberName()
                check(name == memberName(parameter.name.toString()))
                check("set_$name" == setterName(parameter.name.toString()))
                val type = parameter.symbol().mapRustType { t -> t.stripOuter<RustType.Option>() }
                rustTemplate(
                    """
                    /// Sets the value for $name #{extraDocs:W}
                    pub fn $name(mut self, value: impl Into<#{type}>) -> Self {
                        self.$name = Some(value.into());
                        self
                    }

                    /// Sets the value for $name #{extraDocs:W}
                    pub fn set_$name(mut self, param: Option<#{nonOptionalType}>) -> Self {
                        self.$name = param;
                        self
                    }
                    """,
                    "nonOptionalType" to parameter.symbol().mapRustType { it.stripOuter<RustType.Option>() },
                    "type" to type,
                    "extraDocs" to
                        writable {
                            if (parameter.default.isPresent || parameter.documentation.isPresent) {
                                docs("")
                            }
                            parameter.default.orNull()?.also {
                                docs("When unset, this parameter has a default value of `$it`.")
                            }
                            parameter.documentation.orNull()?.also { docs(it) }
                        },
                )
            }
        }
    }
}
