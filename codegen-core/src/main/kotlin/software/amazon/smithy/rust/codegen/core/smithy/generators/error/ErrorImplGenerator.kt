/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asDeref
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.StdError
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.shouldRedact

/** Error customization sections */
sealed class ErrorImplSection(name: String) : Section(name) {
    /** Use this section to add additional trait implementations to the generated error structures */
    class ErrorAdditionalTraitImpls(val errorType: Symbol) : ErrorImplSection("ErrorAdditionalTraitImpls")
}

/** Customizations for generated errors */
abstract class ErrorImplCustomization : NamedCustomization<ErrorImplSection>()

sealed class ErrorKind {
    abstract fun writable(runtimeConfig: RuntimeConfig): Writable

    object Throttling : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ThrottlingError", RuntimeType.retryErrorKind(runtimeConfig)) }
    }

    object Client : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ClientError", RuntimeType.retryErrorKind(runtimeConfig)) }
    }

    object Server : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ServerError", RuntimeType.retryErrorKind(runtimeConfig)) }
    }
}

/**
 * Returns the modeled retryKind for this shape
 *
 * This is _only_ non-null in cases where the @retryable trait has been applied.
 */
fun StructureShape.modeledRetryKind(errorTrait: ErrorTrait): ErrorKind? {
    val retryableTrait = this.getTrait<RetryableTrait>() ?: return null
    return when {
        retryableTrait.throttling -> ErrorKind.Throttling
        errorTrait.isClientError -> ErrorKind.Client
        errorTrait.isServerError -> ErrorKind.Server
        // The error _must_ be either a client or server error
        else -> TODO()
    }
}

class ErrorImplGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
    private val error: ErrorTrait,
    private val customizations: List<ErrorImplCustomization>,
) {
    private val runtimeConfig = symbolProvider.config.runtimeConfig

    fun render(forWhom: CodegenTarget = CodegenTarget.CLIENT) {
        val symbol = symbolProvider.toSymbol(shape)
        val messageShape = shape.errorMessageMember()
        val errorKindT = RuntimeType.retryErrorKind(runtimeConfig)
        writer.rustBlock("impl ${symbol.name}") {
            val retryKindWriteable = shape.modeledRetryKind(error)?.writable(runtimeConfig)
            if (retryKindWriteable != null) {
                rust("/// Returns `Some(${errorKindT.name})` if the error is retryable. Otherwise, returns `None`.")
                rustBlock("pub fn retryable_error_kind(&self) -> #T", errorKindT) {
                    retryKindWriteable(this)
                }
            }
            if (messageShape != null) {
                val messageSymbol = symbolProvider.toSymbol(messageShape).mapRustType { t -> t.asDeref() }
                val messageType = messageSymbol.rustType()
                val memberName = symbolProvider.toMemberName(messageShape)
                val (returnType, message) =
                    if (messageType.stripOuter<RustType.Option>() is RustType.Opaque) {
                        // The string shape has a constraint trait that makes its symbol be a wrapper tuple struct.
                        if (messageSymbol.isOptional()) {
                            "Option<&${messageType.stripOuter<RustType.Option>().render()}>" to
                                "self.$memberName.as_ref()"
                        } else {
                            "&${messageType.render()}" to "&self.$memberName"
                        }
                    } else {
                        if (messageSymbol.isOptional()) {
                            messageType.render() to "self.$memberName.as_deref()"
                        } else {
                            messageType.render() to "self.$memberName.as_ref()"
                        }
                    }

                rust(
                    """
                    /// Returns the error message.
                    pub fn message(&self) -> $returnType { $message }
                    """,
                )
            }

            /*
             * If we're generating for a server, the `name` method is added to enable
             * recording encountered error types inside `http::Extensions`s.
             */
            if (forWhom == CodegenTarget.SERVER) {
                rust(
                    """
                    ##[doc(hidden)]
                    /// Returns the error name.
                    pub fn name(&self) -> &'static str {
                        ${shape.id.name.dq()}
                    }
                    """,
                )
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                // If the error id and the Rust name don't match, print the actual error id for easy debugging
                // Note: Exceptions cannot be renamed so it is OK to not call `getName(service)` here
                val errorDesc =
                    symbol.name.letIf(symbol.name != shape.id.name) { symbolName ->
                        "$symbolName [${shape.id.name}]"
                    }
                write("::std::write!(f, ${errorDesc.dq()})?;")
                messageShape?.let {
                    if (it.shouldRedact(model)) {
                        write("""::std::write!(f, ": {}", $REDACTION)?;""")
                    } else {
                        ifSet(it, symbolProvider.toSymbol(it), ValueExpression.Reference("&self.message")) { field ->
                            val referenced = field.asRef()
                            if (referenced.startsWith("&")) {
                                write("""::std::write!(f, ": {}", $referenced)?;""")
                            } else {
                                write("""::std::write!(f, ": {$referenced}")?;""")
                            }
                        }
                    }
                }
                write("Ok(())")
            }
        }

        writer.write("impl #T for ${symbol.name} {}", StdError)

        writer.writeCustomizations(customizations, ErrorImplSection.ErrorAdditionalTraitImpls(symbol))
    }
}
