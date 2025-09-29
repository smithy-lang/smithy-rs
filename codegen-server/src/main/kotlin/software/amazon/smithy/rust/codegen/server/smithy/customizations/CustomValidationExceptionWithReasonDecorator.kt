/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Length
import software.amazon.smithy.rust.codegen.server.smithy.generators.Pattern
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.isKeyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.isValueConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * A decorator that adds code to convert from constraint violations to a custom `ValidationException` shape that is very
 * similar to `smithy.framework#ValidationException`, with an additional `reason` field.
 *
 * The shape definition is in [CustomValidationExceptionWithReasonDecoratorTest].
 *
 * This is just an example to showcase experimental support for custom validation exceptions.
 * TODO(https://github.com/smithy-lang/smithy-rs/pull/2053): this will go away once we implement the RFC, when users will be
 *  able to define the converters in their Rust application code.
 */
class CustomValidationExceptionWithReasonDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "CustomValidationExceptionWithReasonDecorator"
    override val order: Byte
        get() = -69

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? =
        if (codegenContext.settings.codegenConfig.experimentalCustomValidationExceptionWithReasonPleaseDoNotUse != null) {
            ValidationExceptionWithReasonConversionGenerator(codegenContext)
        } else {
            null
        }
}

class ValidationExceptionWithReasonConversionGenerator(private val codegenContext: ServerCodegenContext) :
    ValidationExceptionConversionGenerator {
    override val shapeId: ShapeId =
        ShapeId.from(codegenContext.settings.codegenConfig.experimentalCustomValidationExceptionWithReasonPleaseDoNotUse)

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
        writable {
            rustTemplate(
                """
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        let first_validation_exception_field = constraint_violation.as_validation_exception_field("".to_owned());
                        let validation_exception = crate::error::ValidationException {
                            message: format!("1 validation error detected. {}", &first_validation_exception_field.message),
                            reason: crate::model::ValidationExceptionReason::FieldValidationFailed,
                            fields: Some(vec![first_validation_exception_field]),
                        };
                        Self::ConstraintViolation(
                            crate::protocol_serde::shape_validation_exception::ser_validation_exception_error(&validation_exception)
                                .expect("validation exceptions should never fail to serialize; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
                        )
                    }
                }
                """,
                "RequestRejection" to protocol.requestRejection(codegenContext.runtimeConfig),
                "From" to RuntimeType.From,
            )
        }

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable =
        writable {
            val validationExceptionFields =
                stringConstraintsInfo.map {
                    writable {
                        when (it) {
                            is Pattern -> {
                                rustTemplate(
                                    """
                                    Self::Pattern(_) => crate::model::ValidationExceptionField {
                                        message: #{MessageWritable:W},
                                        name: path,
                                        reason: crate::model::ValidationExceptionFieldReason::PatternNotValid,
                                    },
                                    """,
                                    "MessageWritable" to it.errorMessage(),
                                )
                            }
                            is Length -> {
                                rust(
                                    """
                                    Self::Length(length) => crate::model::ValidationExceptionField {
                                        message: format!("${it.lengthTrait.validationErrorMessage()}", length, &path),
                                        name: path,
                                        reason: crate::model::ValidationExceptionFieldReason::LengthNotValid,
                                    },
                                    """,
                                )
                            }
                        }
                    }
                }.join("\n")

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to validationExceptionFields,
            )
        }

    override fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait) =
        writable {
            val enumValueSet = enumTrait.enumDefinitionValues.joinToString(", ")
            val message = "Value at '{}' failed to satisfy constraint: Member must satisfy enum value set: [$enumValueSet]"
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                    crate::model::ValidationExceptionField {
                        message: format!(r##"$message"##, &path),
                        name: path,
                        reason: crate::model::ValidationExceptionFieldReason::ValueNotValid,
                    }
                }
                """,
                "String" to RuntimeType.String,
            )
        }

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range) =
        writable {
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                    match self {
                        Self::Range(_) => crate::model::ValidationExceptionField {
                            message: format!("${rangeInfo.rangeTrait.validationErrorMessage()}", &path),
                            name: path,
                            reason: crate::model::ValidationExceptionFieldReason::ValueNotValid,
                        }
                    }
                }
                """,
                "String" to RuntimeType.String,
            )
        }

    override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>) =
        writable {
            val validationExceptionFields =
                blobConstraintsInfo.map {
                    writable {
                        rust(
                            """
                            Self::Length(length) => crate::model::ValidationExceptionField {
                                message: format!("${it.lengthTrait.validationErrorMessage()}", length, &path),
                                name: path,
                                reason: crate::model::ValidationExceptionFieldReason::LengthNotValid,
                            },
                            """,
                        )
                    }
                }.join("\n")

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to validationExceptionFields,
            )
        }

    override fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ) = writable {
        rustBlockTemplate(
            "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
            "String" to RuntimeType.String,
        ) {
            rustBlock("match self") {
                shape.getTrait<LengthTrait>()?.also {
                    rust(
                        """
                        Self::Length(length) => crate::model::ValidationExceptionField {
                            message: format!("${it.validationErrorMessage()}", length, &path),
                            name: path,
                            reason: crate::model::ValidationExceptionFieldReason::LengthNotValid,
                        },
                        """,
                    )
                }
                if (isKeyConstrained(keyShape, symbolProvider)) {
                    rust("""Self::Key(key_constraint_violation) => key_constraint_violation.as_validation_exception_field(path),""")
                }
                if (isValueConstrained(valueShape, model, symbolProvider)) {
                    rust("""Self::Value(key, value_constraint_violation) => value_constraint_violation.as_validation_exception_field(path + "/" + key.as_str()),""")
                }
            }
        }
    }

    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>) =
        writable {
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rust(
                                """
                                ConstraintViolation::${it.name()} => crate::model::ValidationExceptionField {
                                    message: format!("Value at '{}/${it.forMember.memberName}' failed to satisfy constraint: Member must not be null", path),
                                    name: path + "/${it.forMember.memberName}",
                                    reason: crate::model::ValidationExceptionFieldReason::Other,
                                },
                                """,
                            )
                        }
                    }
                }
            }
        }

    override fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ) = writable {
        val validationExceptionFields =
            collectionConstraintsInfo.map {
                writable {
                    when (it) {
                        is CollectionTraitInfo.Length -> {
                            rust(
                                """
                                Self::Length(length) => crate::model::ValidationExceptionField {
                                    message: format!("${it.lengthTrait.validationErrorMessage()}", length, &path),
                                    name: path,
                                    reason: crate::model::ValidationExceptionFieldReason::LengthNotValid,
                                },
                                """,
                            )
                        }
                        is CollectionTraitInfo.UniqueItems -> {
                            rust(
                                """
                                Self::UniqueItems { duplicate_indices, .. } =>
                                    crate::model::ValidationExceptionField {
                                        message: format!("${it.uniqueItemsTrait.validationErrorMessage()}", &duplicate_indices, &path),
                                        name: path,
                                        reason: crate::model::ValidationExceptionFieldReason::ValueNotValid,
                                    },
                                """,
                            )
                        }
                    }
                }
            }.toMutableList()

        if (isMemberConstrained) {
            validationExceptionFields += {
                rust(
                    """
                    Self::Member(index, member_constraint_violation) =>
                        member_constraint_violation.as_validation_exception_field(path + "/" + &index.to_string())
                    """,
                )
            }
        }
        rustTemplate(
            """
            pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                match self {
                    #{AsValidationExceptionFields:W}
                }
            }
            """,
            "String" to RuntimeType.String,
            "AsValidationExceptionFields" to validationExceptionFields.join("\n"),
        )
    }

    override fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ) = writable {
        rustBlockTemplate(
            "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
            "String" to RuntimeType.String,
        ) {
            withBlock("match self {", "}") {
                for (constraintViolation in unionConstraintTraitInfo) {
                    rust("""Self::${constraintViolation.name()}(inner) => inner.as_validation_exception_field(path + "/${constraintViolation.forMember.memberName}"),""")
                }
            }
        }
    }
}
