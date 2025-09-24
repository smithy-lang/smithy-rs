package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
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
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.TraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.isKeyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.isValueConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage
import software.amazon.smithy.rust.codegen.server.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationFieldNameTrait
import kotlin.collections.forEach

// Extension property to get custom exception name from model
val ServerCodegenContext.customExceptionName: String?
    get() = model.shapes(StructureShape::class.java)
        .filter { it.hasTrait(ValidationExceptionTrait.ID) }
        .findFirst()
        .orElse(null)
        ?.id?.name

val ServerCodegenContext.customExceptionFieldName: String?
    get() =
        // Find the structure that contains a member with @validationFieldName trait
        model.shapes(StructureShape::class.java)
            .filter { structure ->
                structure.members().any { member ->
                    member.hasTrait(ValidationFieldNameTrait.ID)
                }
            }
            .findFirst()
            .orElse(null)
            ?.id?.name

// Shared utility class for generating ValidationExceptionField structures and conversion methods
class ValidationExceptionDecoratorGenerator(private val codegenContext: ServerCodegenContext) {
    val exceptionName = codegenContext.customExceptionName ?: "ValidationException"
    val exceptionFieldName = codegenContext.customExceptionFieldName ?: "ValidationExceptionField"

    fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
        writable {
            rustTemplate(
                """
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        let first_validation_exception_field = constraint_violation.as_validation_exception_field("".to_owned());
                        let validation_exception = crate::error::$exceptionName {
                            message: format!("1 validation error detected. {}", &first_validation_exception_field.message),
                            field_list: Some(vec![first_validation_exception_field]),
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

    fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable =
        writable {
            val constraintsInfo: List<TraitInfo> = stringConstraintsInfo.map(StringTraitInfo::toTraitInfo)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName {
                    match self {
                        #{ValidationExceptionFields:W}
                    }`
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to constraintsInfo.map { it.asValidationExceptionField }.join("\n"),
            )
        }

    fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable =
        writable {
            val constraintsInfo: List<TraitInfo> = blobConstraintsInfo.map(BlobLength::toTraitInfo)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to constraintsInfo.map { it.asValidationExceptionField }.join("\n"),
            )
        }

    fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ): Writable = writable {
        rustBlockTemplate(
            "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName",
            "String" to RuntimeType.String,
        ) {
            rustBlock("match self") {
                shape.getTrait<LengthTrait>()?.also {
                    rust(
                        """
                        Self::Length(length) => crate::model::$exceptionFieldName {
                            message: format!("${it.validationErrorMessage()}", length, &path),
                            path,
                        },""",
                    )
                }
                if (isKeyConstrained(keyShape, symbolProvider)) {
                    // Note how we _do not_ append the key's member name to the path. This is intentional, as
                    // per the `RestJsonMalformedLengthMapKey` test. Note keys are always strings.
                    // https://github.com/awslabs/smithy/blob/ee0b4ff90daaaa5101f32da936c25af8c91cc6e9/smithy-aws-protocol-tests/model/restJson1/validation/malformed-length.smithy#L296-L295
                    rust("""Self::Key(key_constraint_violation) => key_constraint_violation.as_validation_exception_field(path),""")
                }
                if (isValueConstrained(valueShape, model, symbolProvider)) {
                    // `as_str()` works with regular `String`s and constrained string shapes.
                    rust("""Self::Value(key, value_constraint_violation) => value_constraint_violation.as_validation_exception_field(path + "/" + key.as_str()),""")
                }
            }
        }
    }

    fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait): Writable =
        writable {
            val message = enumTrait.validationErrorMessage()
            // TODO: fix fields
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName {
                    crate::model::$exceptionFieldName {
                        message: format!(r##"$message"##, &path),
                        path,
                    }
                }
                """,
                "String" to RuntimeType.String,
            )
        }

    fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable =
        writable {
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "ValidationExceptionFields" to rangeInfo.toTraitInfo().asValidationExceptionField,
            )
        }


    fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable =
        writable {
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName",
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rust(
                                """
                                ConstraintViolation::${it.name()} => crate::model::$exceptionFieldName {
                                    message: format!("Value at '{}/${it.forMember.memberName}' failed to satisfy constraint: Member must not be null", path),
                                    path: path + "/${it.forMember.memberName}",
                                },
                                """,
                            )
                        }
                    }
                }
            }
        }

    fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ): Writable = writable {
        val validationExceptionFields =
            collectionConstraintsInfo.map {
                it.toTraitInfo().asValidationExceptionField
            }.toMutableList()
        if (isMemberConstrained) {
            validationExceptionFields += {
                rust(
                    """Self::Member(index, member_constraint_violation) =>
                    member_constraint_violation.as_validation_exception_field(path + "/" + &index.to_string())
                    """,
                )
            }
        }
        rustTemplate(
            """
            pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName {
                match self {
                    #{AsValidationExceptionFields:W}
                }
            }
            """,
            "String" to RuntimeType.String,
            "AsValidationExceptionFields" to validationExceptionFields.join(""),
        )
    }

    fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ): Writable = writable {
        rustBlockTemplate(
            "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::$exceptionFieldName",
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
