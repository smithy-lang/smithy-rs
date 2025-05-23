/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGeneratorContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumMemberModel
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumType
import software.amazon.smithy.rust.codegen.core.util.dq

/** Infallible enums have an `Unknown` variant and can't fail to parse */
data class InfallibleEnumType(
    val unknownVariantModule: RustModule,
) : EnumType() {
    companion object {
        /** Name of the generated unknown enum member name for enums with named members. */
        const val UNKNOWN_VARIANT = "Unknown"

        /** Name of the opaque struct that is inner data for the generated [UNKNOWN_VARIANT]. */
        const val UNKNOWN_VARIANT_VALUE = "UnknownVariantValue"
    }

    override fun implFromForStr(context: EnumGeneratorContext): Writable =
        writable {
            rustTemplate(
                """
                impl #{From}<&str> for ${context.enumName} {
                    fn from(s: &str) -> Self {
                        match s {
                            #{matchArms}
                        }
                    }
                }
                """,
                "From" to RuntimeType.From,
                "matchArms" to
                    writable {
                        context.sortedMembers.forEach { member ->
                            rust("${member.value.dq()} => ${context.enumName}::${member.derivedName()},")
                        }
                        rust(
                            "other => ${context.enumName}::$UNKNOWN_VARIANT(#T(other.to_owned()))",
                            unknownVariantValue(context),
                        )
                    },
            )
        }

    override fun implFromStr(context: EnumGeneratorContext): Writable =
        writable {
            rustTemplate(
                """
                impl ::std::str::FromStr for ${context.enumName} {
                    type Err = ::std::convert::Infallible;

                    fn from_str(s: &str) -> #{Result}<Self, <Self as ::std::str::FromStr>::Err> {
                        #{Ok}(${context.enumName}::from(s))
                    }
                }
                """,
                *preludeScope,
            )
        }

    override fun implFromForStrForUnnamedEnum(context: EnumGeneratorContext): Writable =
        writable {
            rustTemplate(
                """
                impl<T> #{From}<T> for ${context.enumName} where T: #{AsRef}<str> {
                    fn from(s: T) -> Self {
                        ${context.enumName}(s.as_ref().to_owned())
                    }
                }
                """,
                *preludeScope,
            )
        }

    override fun implFromStrForUnnamedEnum(context: EnumGeneratorContext): Writable =
        writable {
            // Add an infallible FromStr implementation for uniformity
            rustTemplate(
                """
                impl ::std::str::FromStr for ${context.enumName} {
                    type Err = ::std::convert::Infallible;

                    fn from_str(s: &str) -> #{Result}<Self, <Self as ::std::str::FromStr>::Err> {
                        #{Ok}(${context.enumName}::from(s))
                    }
                }
                """,
                *preludeScope,
            )
        }

    override fun additionalEnumImpls(context: EnumGeneratorContext): Writable =
        writable {
            // `try_parse` isn't needed for unnamed enums
            if (context.enumTrait.hasNames()) {
                rustTemplate(
                    """
                    impl ${context.enumName} {
                        /// Parses the enum value while disallowing unknown variants.
                        ///
                        /// Unknown variants will result in an error.
                        pub fn try_parse(value: &str) -> #{Result}<Self, #{UnknownVariantError}> {
                            match Self::from(value) {
                                ##[allow(deprecated)]
                                Self::Unknown(_) => #{Err}(#{UnknownVariantError}::new(value)),
                                known => Ok(known),
                            }
                        }
                    }
                    """,
                    *preludeScope,
                    "UnknownVariantError" to unknownVariantError(),
                )

                rustTemplate(
                    """
                    impl #{Display} for ${context.enumName} {
                        fn fmt(&self, f: &mut #{Fmt}::Formatter) -> #{Fmt}::Result {
                            match self {
                                #{matchArms}
                            }
                        }
                    }
                    """,
                    "Display" to RuntimeType.Display,
                    "Fmt" to RuntimeType.stdFmt,
                    "matchArms" to
                        writable {
                            context.sortedMembers.forEach { member ->
                                rust(
                                    """
                                    ${context.enumName}::${member.derivedName()} => write!(f, ${member.value.dq()}),
                                    """,
                                )
                            }
                            rust("""${context.enumName}::Unknown(value) => write!(f, "{}", value)""")
                        },
                )
            }
        }

    override fun additionalDocs(context: EnumGeneratorContext): Writable =
        writable {
            renderForwardCompatibilityNote(
                context.enumName,
                context.sortedMembers,
                UNKNOWN_VARIANT,
                UNKNOWN_VARIANT_VALUE,
            )
        }

    override fun additionalEnumMembers(context: EnumGeneratorContext): Writable =
        writable {
            docs("`$UNKNOWN_VARIANT` contains new variants that have been added since this code was generated.")
            rust(
                """##[deprecated(note = "Don't directly match on `$UNKNOWN_VARIANT`. See the docs on this enum for the correct way to handle unknown variants.")]""",
            )
            rust("$UNKNOWN_VARIANT(#T)", unknownVariantValue(context))
        }

    override fun additionalAsStrMatchArms(context: EnumGeneratorContext): Writable =
        writable {
            rust("${context.enumName}::$UNKNOWN_VARIANT(value) => value.as_str()")
        }

    private fun unknownVariantValue(context: EnumGeneratorContext): RuntimeType {
        return RuntimeType.forInlineFun(UNKNOWN_VARIANT_VALUE, unknownVariantModule) {
            docs(
                """
                Opaque struct used as inner data for the `Unknown` variant defined in enums in
                the crate.

                This is not intended to be used directly.
                """.trimIndent(),
            )

            // The UnknownVariant's underlying opaque type should always derive `Debug`. If this isn't explicitly
            // added and the first enum to render this module has the `@sensitive` trait then the UnknownVariant
            // inherits that sensitivity and does not derive `Debug`. This leads to issues deriving `Debug` for
            // all enum variants that are not marked `@sensitive`.
            context.enumMeta.withDerives(RuntimeType.Debug).render(this)
            rustTemplate("struct $UNKNOWN_VARIANT_VALUE(pub(crate) #{String});", *preludeScope)
            rustBlock("impl $UNKNOWN_VARIANT_VALUE") {
                // The generated as_str is not pub as we need to prevent users from calling it on this opaque struct.
                rustBlock("pub(crate) fn as_str(&self) -> &str") {
                    rust("&self.0")
                }
            }
            rustTemplate(
                """
                impl #{Display} for $UNKNOWN_VARIANT_VALUE {
                    fn fmt(&self, f: &mut #{Fmt}::Formatter) -> #{Fmt}::Result {
                        write!(f, "{}", self.0)
                    }
                }
                """,
                "Display" to RuntimeType.Display,
                "Fmt" to RuntimeType.stdFmt,
            )
        }
    }

    /**
     * Generate the rustdoc describing how to write a match expression against a generated enum in a
     * forward-compatible way.
     */
    private fun RustWriter.renderForwardCompatibilityNote(
        enumName: String,
        sortedMembers: List<EnumMemberModel>,
        unknownVariant: String,
        unknownVariantValue: String,
    ) {
        docs(
            """
            When writing a match expression against `$enumName`, it is important to ensure
            your code is forward-compatible. That is, if a match arm handles a case for a
            feature that is supported by the service but has not been represented as an enum
            variant in a current version of SDK, your code should continue to work when you
            upgrade SDK to a future version in which the enum does include a variant for that
            feature.
            """.trimIndent(),
        )
        docs("")
        docs("Here is an example of how you can make a match expression forward-compatible:")
        docs("")
        docs("```text")
        rust("/// ## let ${enumName.lowercase()} = unimplemented!();")
        rust("/// match ${enumName.lowercase()} {")
        sortedMembers.mapNotNull { it.name() }.forEach { member ->
            rust("///     $enumName::${member.name} => { /* ... */ },")
        }
        rust("""///     other @ _ if other.as_str() == "NewFeature" => { /* handles a case for `NewFeature` */ },""")
        rust("///     _ => { /* ... */ },")
        rust("/// }")
        docs("```")
        docs(
            """
            The above code demonstrates that when `${enumName.lowercase()}` represents
            `NewFeature`, the execution path will lead to the second last match arm,
            even though the enum does not contain a variant `$enumName::NewFeature`
            in the current version of SDK. The reason is that the variable `other`,
            created by the `@` operator, is bound to
            `$enumName::$unknownVariant($unknownVariantValue("NewFeature".to_owned()))`
            and calling `as_str` on it yields `"NewFeature"`.
            This match expression is forward-compatible when executed with a newer
            version of SDK where the variant `$enumName::NewFeature` is defined.
            Specifically, when `${enumName.lowercase()}` represents `NewFeature`,
            the execution path will hit the second last match arm as before by virtue of
            calling `as_str` on `$enumName::NewFeature` also yielding `"NewFeature"`.
            """.trimIndent(),
        )
        docs("")
        docs(
            """
            Explicitly matching on the `$unknownVariant` variant should
            be avoided for two reasons:
            - The inner data `$unknownVariantValue` is opaque, and no further information can be extracted.
            - It might inadvertently shadow other intended match arms.
            """.trimIndent(),
        )
        docs("")
    }
}

class ClientEnumGenerator(codegenContext: ClientCodegenContext, shape: StringShape) :
    EnumGenerator(
        codegenContext.model,
        codegenContext.symbolProvider,
        shape,
        InfallibleEnumType(
            RustModule.new(
                "sealed_enum_unknown",
                visibility = Visibility.PUBCRATE,
                parent = ClientRustModule.primitives,
            ),
        ),
    )

private fun unknownVariantError(): RuntimeType =
    RuntimeType.forInlineFun("UnknownVariantError", ClientRustModule.Error) {
        rustTemplate(
            """
            /// The given enum value failed to parse since it is not a known value.
            ##[derive(Debug)]
            pub struct UnknownVariantError {
                value: #{String},
            }
            impl UnknownVariantError {
                pub(crate) fn new(value: impl #{Into}<#{String}>) -> Self {
                    Self { value: value.into() }
                }
            }
            impl ::std::fmt::Display for UnknownVariantError {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> #{Result}<(), ::std::fmt::Error> {
                    write!(f, "unknown enum variant: '{}'", self.value)
                }
            }
            impl ::std::error::Error for UnknownVariantError {}
            """,
            *preludeScope,
        )
    }
