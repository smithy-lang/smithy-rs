package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.rulesengine.language.eval.Value
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.implInto
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustInline
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

class EndpointParamsGenerator(
    private val parameters: List<Parameter>,
) {
    val error = RuntimeType.forInlineFun("Error", EndpointsModule) {
        it.docs("An error that occurred during endpoint resolution")
        Attribute.NonExhaustive.render(it)
        Attribute.Derives(setOf(RuntimeType.Debug)).render(it)
        it.rust(
            """
            pub enum Error {
                ##[non_exhaustive]
                /// A required field was missing
                MissingRequiredField {
                    /// Name of the missing field
                    field: std::borrow::Cow<'static, str>
                },

                ##[non_exhaustive]
                /// A valid endpoint could not be resolved
                EndpointResolutionError {
                    /// The error message
                    message: std::borrow::Cow<'static, str>
                }
            }

            impl Error {
                ##[allow(dead_code)]
                fn missing(field: &'static str) -> Self {
                    Self::MissingRequiredField { field: field.into() }
                }
                
                ##[allow(dead_code)]
                fn endpoint_resolution(message: std::borrow::Cow<'static, str>) -> Self {
                    Self::EndpointResolutionError { message }
                }
            }

            impl std::fmt::Display for Error {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        Error::MissingRequiredField { field } => write!(f, "A required field was missing: `{}`", field),
                        Error::EndpointResolutionError { message } => write!(f, "A valid endpoint could not be resolved: {}", message)
                    }
                }
            }

            impl std::error::Error for Error { }
            """,
        )
    }

    val params = RuntimeType.forInlineFun("Params", EndpointsModule) { writer ->
        // Ensure that fields can be added in the future
        Attribute.NonExhaustive.render(writer)
        // Automatically implement standard Rust functionality
        Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.PartialEq, RuntimeType.Clone)).render(writer)
        // Generate the struct block:
        /*
            pub struct Params {
                ... members: pub(crate) field
            }
        */
        writer.docs("Configuration parameters for resolving the correct endpoint")
        writer.rustBlock("pub struct Params") {
            parameters.forEach { parameter ->
                // Render documentation for each parameter
                parameter.documentation.orNull()?.also { docs(it) }
                writer.rust("pub(crate) ${parameter.memberName()}: #T,", parameter.symbol())
            }
        }

        // Generate the impl block for the struct
        writer.rustBlock("impl Params") {
            rust("pub fn builder() -> #T { Default::default() }", builder)
        }
    }

    val builder = RuntimeType.forInlineFun("Builder", EndpointsModule) { writer ->
        writer.docs("Builder for [`Params`]")
        Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.Default, RuntimeType.PartialEq, RuntimeType.Clone))
            .render(writer)

        // builder struct declaration
        writer.rustBlock("pub struct Builder") {
            parameters.forEach { parameter ->
                val name = parameter.memberName()
                val type = parameter.symbol().makeOptional()
                rust("$name: #T,", type)
            }
        }

        // builder struct impl
        writer.rustBlock("impl Builder") {
            writer.rustTemplate(
                """
                #{build_method:W}
                #{setter_methods:W}
                """,
                "build_method" to generateEndpointBuilderBuildMethod(),
                "setter_methods" to generateEndpointBuilderSetters(),
            )
        }
    }

    private fun value(value: Value): String {
        return when (value) {
            is Value.String -> value.value().dq() + ".to_string()"
            is Value.Bool -> value.expectBool().toString()
            else -> TODO("unexpected type: $value")
        }
    }

    private fun generateEndpointBuilderBuildMethod() = writable {
        docs("Consume this builder, creating [`Params`].")
        rustBlockTemplate(
            "pub fn build(self) -> Result<Params, Error>",
        ) {
            val params = writable {
                rustBlockTemplate("Params") {
                    parameters.forEach { parameter ->
                        rust("${parameter.memberName()}: self.${parameter.memberName()}")
                        parameter.default.orNull()?.also { default -> rust(".or(Some(${value(default)}))") }
                        if (parameter.isRequired) {
                            rust(".ok_or_else(|| Error::missing(${parameter.memberName().dq()}))?")
                        }
                        rust(",")
                    }
                }
            }
            rust("Ok(#W)", params)
        }
    }

    private fun generateEndpointBuilderSetters() = writable {
        parameters.forEach { parameter ->
            val name = parameter.memberName()

            check(name == parameter.name.toRustName())
            check("set_$name" == "set_${parameter.name.toRustName()}")

            val (type, value) = parameter.symbol().rustType().stripOuter<RustType.Option>().let { t ->
                when (t) {
                    // `impl Into` allows the function to accept both `&str`s and `String`s
                    RustType.String -> writable { rustInline(t.implInto()) } to "value.into()"
                    else -> writable { rustInline("#T", t) } to "value"
                }
            }
            val extraDocs = writable {
                if (parameter.default.isPresent || parameter.documentation.isPresent) {
                    docs("")
                }
                parameter.default.orNull()?.also {
                    docs("When unset, this parameter has a default value of `$it`.")
                }
                parameter.documentation.orNull()?.also { docs(it) }
            }
            rustTemplate(
                """
                    /// Sets the value for $name #{extraDocs:W}
                    pub fn $name(mut self, value: #{type:W}) -> Self {
                        self.$name = Some($value);
                        self
                    }

                    /// Sets the value for $name #{extraDocs:W}
                    pub fn set_$name(mut self, param: Option<impl Into<#{nonOptionalType}>>) -> Self {
                        self.$name = param.map(|t|t.into());
                        self
                    }
                    """,
                "nonOptionalType" to parameter.symbol().mapRustType { it.stripOuter<RustType.Option>() },
                "type" to type,
                "extraDocs" to extraDocs,
            )
        }
    }
}
