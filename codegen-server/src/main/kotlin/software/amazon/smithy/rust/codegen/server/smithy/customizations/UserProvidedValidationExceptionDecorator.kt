/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.targetOrSelf
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.isKeyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.isValueConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.util.isValidationMessage
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage
import software.amazon.smithy.rust.codegen.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldListTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldMessageTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldNameTrait

/**
 * Decorator for user provided validation exception codegen
 *
 * The order of this is less than that of [SmithyValidationExceptionDecorator] so it takes precedence regardless of the
 * order decorators are passed into the plugin
 */
class UserProvidedValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "UserProvidedValidationExceptionDecorator"
    override val order: Byte
        get() = 68

    internal fun userProvidedValidationException(codegenContext: ServerCodegenContext): StructureShape? =
        codegenContext.model
            .shapes(StructureShape::class.java)
            .toList()
            // Defining multiple validation exceptions is unsupported. See `ValidateUnsupportedConstraints`
            .firstOrNull({ it.hasTrait(ValidationExceptionTrait.ID) })

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? =
        userProvidedValidationException(codegenContext)?.let {
            UserProvidedValidationExceptionConversionGenerator(codegenContext, it)
        }
}

class UserProvidedValidationExceptionConversionGenerator(
    private val codegenContext: ServerCodegenContext,
    private val validationException: StructureShape,
) : ValidationExceptionConversionGenerator {
    private val maybeValidationField = userProvidedValidationField()

    private val codegenCtx =
        listOfNotNull(maybeValidationField)
            .map {
                "UserProvidedValidationExceptionField" to codegenContext.symbolProvider.toSymbol(it)
            }.toTypedArray()

    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#UserProvidedValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID

    internal fun userProvidedValidationMessage(): MemberShape =
        validationException
            .members()
            .firstOrNull { it.isValidationMessage() }
            ?: throw CodegenException("Expected $validationException to contain a member with ValidationMessageTrait")

    internal fun userProvidedValidationFieldList(): MemberShape? =
        validationException
            .members()
            .firstOrNull { it.hasTrait(ValidationFieldListTrait.ID) }

    internal fun userProvidedValidationField(): StructureShape? {
        val validationFieldListMember = userProvidedValidationFieldList() ?: return null

        // get target of member of the user provided validation field list that represents the structure for the
        // validation field shape, otherwise, we return null as field list is optional
        val validationFieldShape =
            validationFieldListMember
                .targetOrSelf(codegenContext.model)
                .members()
                .firstOrNull { it.targetOrSelf(codegenContext.model).isStructureShape }
                ?.targetOrSelf(codegenContext.model)
                ?.asStructureShape()
                ?.orElse(null)
                ?: return null

        // It is required that a member of the user provided validation field structure has @validationFieldName
        if (validationFieldShape
                .members()
                .none { it.hasTrait(ValidationFieldNameTrait.ID) }
        ) {
            throw CodegenException("Expected $validationFieldShape to contain a member with ValidationFieldNameTrait")
        }

        return validationFieldShape
    }

    internal fun userProvidedValidationFieldMessage(): MemberShape? {
        val validationField = userProvidedValidationField() ?: return null

        return validationField.members().firstOrNull { it.hasTrait(ValidationFieldMessageTrait.ID) }
    }

    internal fun userProvidedValidationAdditionalFields(): List<MemberShape> =
        validationException.members().filter { member ->
            !member.isValidationMessage() && !member.hasTrait(ValidationFieldListTrait.ID)
        }

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable {
        val validationMessage = userProvidedValidationMessage()
        val validationFieldList = userProvidedValidationFieldList()
        val validationFieldMessage = userProvidedValidationFieldMessage()
        val additionalFields = userProvidedValidationAdditionalFields()

        return writable {
            val validationMessageName = codegenContext.symbolProvider.toMemberName(validationMessage)!!
            // Generate the correct shape module name for the user provided validation exception
            val shapeModuleName =
                codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, validationException)
            val shapeFunctionName = validationException.id.name.toSnakeCase()

            rustTemplate(
                """
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        #{FieldCreation}
                        let validation_exception = #{UserProvidedValidationException} {
                            $validationMessageName: #{ValidationMessage},
                            #{FieldListAssignment}
                            #{AdditionalFieldAssignments}
                        };
                        Self::ConstraintViolation(
                            crate::protocol_serde::$shapeModuleName::ser_${shapeFunctionName}_error(&validation_exception)
                                .expect("validation exceptions should never fail to serialize; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
                        )
                    }
                }
                """,
                *preludeScope,
                "RequestRejection" to protocol.requestRejection(codegenContext.runtimeConfig),
                "UserProvidedValidationException" to codegenContext.symbolProvider.toSymbol(validationException),
                "FieldCreation" to
                    writable {
                        if (validationFieldList != null) {
                            rust("""let first_validation_exception_field = constraint_violation.as_validation_exception_field("".to_owned());""")
                        }
                    },
                "ValidationMessage" to
                    writable {
                        val message =
                            if (validationFieldList != null && validationFieldMessage != null) {
                                val validationFieldMessageName =
                                    codegenContext.symbolProvider.toMemberName(validationFieldMessage)!!
                                if (validationFieldMessage.isOptional) {
                                    """format!("validation error detected. {}", &first_validation_exception_field.$validationFieldMessageName.clone().unwrap_or_default())"""
                                } else {
                                    """format!("validation error detected. {}", &first_validation_exception_field.$validationFieldMessageName)"""
                                }
                            } else {
                                """format!("validation error detected")"""
                            }
                        if (validationMessage.isOptional) {
                            rust("Some($message)")
                        } else {
                            rust(message)
                        }
                    },
                "FieldListAssignment" to
                    writable {
                        if (validationFieldList != null) {
                            val fieldName = codegenContext.symbolProvider.toMemberName(validationFieldList)!!
                            val value = "vec![first_validation_exception_field]"
                            if (validationFieldList.isOptional) {
                                rust("$fieldName: Some($value),")
                            } else {
                                rust("$fieldName: $value,")
                            }
                        }
                    },
                "AdditionalFieldAssignments" to
                    writable {
                        rust(
                            additionalFields.joinToString { member ->
                                val memberName = codegenContext.symbolProvider.toMemberName(member)!!
                                "$memberName: ${defaultFieldAssignment(member)}"
                            },
                        )
                    },
            )
        }
    }

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenCtx,
                "ValidationExceptionFields" to
                    writable {
                        stringConstraintsInfo.forEach { stringTraitInfo ->
                            when (stringTraitInfo::class.simpleName) {
                                "Length" -> {
                                    val lengthTrait =
                                        stringTraitInfo::class.java
                                            .getDeclaredField("lengthTrait")
                                            .apply { isAccessible = true }
                                            .get(stringTraitInfo) as LengthTrait
                                    rustTemplate(
                                        """
                                        Self::Length(length) => #{UserProvidedValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenCtx,
                                        "FieldAssignments" to
                                            fieldAssignments(
                                                "path.clone()",
                                                """format!(${
                                                    lengthTrait.validationErrorMessage().dq()
                                                }, length, &path)""",
                                            ),
                                    )
                                }

                                "Pattern" -> {
                                    val patternTrait =
                                        stringTraitInfo::class.java
                                            .getDeclaredField("patternTrait")
                                            .apply { isAccessible = true }
                                            .get(stringTraitInfo) as PatternTrait
                                    rustTemplate(
                                        """
                                        Self::Pattern(_) => #{UserProvidedValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenCtx,
                                        "FieldAssignments" to
                                            fieldAssignments(
                                                "path.clone()",
                                                """format!(${
                                                    patternTrait.validationErrorMessage().dq()
                                                }, &path, ${patternTrait.pattern.toString().dq()})""",
                                            ),
                                    )
                                }
                            }
                        }
                    },
            )
        }
    }

    override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenCtx,
                "ValidationExceptionFields" to
                    writable {
                        val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)
                        blobConstraintsInfo.forEach { blobLength ->
                            rustTemplate(
                                """
                                Self::Length(length) => #{UserProvidedValidationExceptionField} {
                                    #{FieldAssignments}
                                },
                                """,
                                *codegenCtx,
                                "FieldAssignments" to
                                    fieldAssignments(
                                        "path.clone()",
                                        """format!(${
                                            blobLength.lengthTrait.validationErrorMessage().dq()
                                        }, length, &path)""",
                                    ),
                            )
                        }
                    },
            )
        }
    }

    override fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField}",
                *preludeScope,
                *codegenCtx,
            ) {
                rustBlock("match self") {
                    shape.getTrait<LengthTrait>()?.also {
                        rustTemplate(
                            """
                            Self::Length(length) => #{UserProvidedValidationExceptionField} {
                                #{FieldAssignments}
                            },""",
                            *codegenCtx,
                            "FieldAssignments" to
                                fieldAssignments(
                                    "path.clone()",
                                    """format!(${it.validationErrorMessage().dq()}, length, &path)""",
                                ),
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
    }

    override fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)
            val message = enumTrait.validationErrorMessage()

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField} {
                    #{UserProvidedValidationExceptionField} {
                        #{FieldAssignments}
                    }
                }
                """,
                *preludeScope,
                *codegenCtx,
                "FieldAssignments" to fieldAssignments("path.clone()", """format!(r##"$message"##, &path)"""),
            )
        }
    }

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField} {
                    match self {
                        Self::Range(_) => #{UserProvidedValidationExceptionField} {
                            #{FieldAssignments}
                        },
                    }
                }
                """,
                *preludeScope,
                *codegenCtx,
                "FieldAssignments" to
                    fieldAssignments(
                        "path.clone()",
                        """format!(${rangeInfo.rangeTrait.validationErrorMessage().dq()}, &path)""",
                    ),
            )
        }
    }

    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField}",
                *preludeScope,
                *codegenCtx,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rustTemplate(
                                """
                                ConstraintViolation::${it.name()} => #{UserProvidedValidationExceptionField} {
                                    #{FieldAssignments}
                                },
                                """.trimIndent(),
                                *codegenCtx,
                                "FieldAssignments" to
                                    fieldAssignments(
                                        """path.clone() + "/${it.forMember.memberName}"""",
                                        """format!("Value at '{}/${it.forMember.memberName}' failed to satisfy constraint: Member must not be null", path)""",
                                    ),
                            )
                        }
                    }
                }
            }
        }
    }

    override fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationField)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenCtx,
                "ValidationExceptionFields" to
                    writable {
                        collectionConstraintsInfo.forEach { collectionTraitInfo ->
                            when (collectionTraitInfo) {
                                is CollectionTraitInfo.Length -> {
                                    rustTemplate(
                                        """
                                        Self::Length(length) => #{UserProvidedValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenCtx,
                                        "FieldAssignments" to
                                            fieldAssignments(
                                                "path.clone()",
                                                """format!(${
                                                    collectionTraitInfo.lengthTrait.validationErrorMessage()
                                                        .dq()
                                                }, length, &path)""",
                                            ),
                                    )
                                }

                                is CollectionTraitInfo.UniqueItems -> {
                                    rustTemplate(
                                        """
                                        Self::UniqueItems { duplicate_indices, .. } => #{UserProvidedValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenCtx,
                                        "FieldAssignments" to
                                            fieldAssignments(
                                                "path.clone()",
                                                """format!(${
                                                    collectionTraitInfo.uniqueItemsTrait.validationErrorMessage()
                                                        .dq()
                                                }, &duplicate_indices, &path)""",
                                            ),
                                    )
                                }
                            }
                        }

                        if (isMemberConstrained) {
                            rust(
                                """Self::Member(index, member_constraint_violation) =>
                                member_constraint_violation.as_validation_exception_field(path + "/" + &index.to_string())
                                """,
                            )
                        }
                    },
            )
        }
    }

    override fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{UserProvidedValidationExceptionField}",
                *preludeScope,
                *codegenCtx,
            ) {
                withBlock("match self {", "}") {
                    for (constraintViolation in unionConstraintTraitInfo) {
                        rust("""Self::${constraintViolation.name()}(inner) => inner.as_validation_exception_field(path + "/${constraintViolation.forMember.memberName}"),""")
                    }
                }
            }
        }
    }

    /**
     * Helper function to generate field assignments for user provided validation exception fields
     */
    private fun generateUserProvidedValidationFieldAssignments(
        userProvidedValidationExceptionField: StructureShape,
    ): (String, String) -> Writable =
        { rawPathExpression: String, rawMessageExpression: String ->
            writable {
                rustTemplate(
                    userProvidedValidationExceptionField.members().joinToString(",") { member ->
                        val memberName = codegenContext.symbolProvider.toMemberName(member)
                        val pathExpression =
                            if (member.isOptional) "Some($rawPathExpression)" else rawPathExpression
                        val messageExpression =
                            if (member.isOptional) "Some($rawMessageExpression)" else rawMessageExpression
                        when {
                            member.hasTrait(ValidationFieldNameTrait.ID) ->
                                "$memberName: $pathExpression"

                            member.hasTrait(ValidationFieldMessageTrait.ID) ->
                                "$memberName: $messageExpression"

                            else -> {
                                "$memberName: ${defaultFieldAssignment(member)}"
                            }
                        }
                    },
                )
            }
        }

    private fun defaultFieldAssignment(member: MemberShape): String {
        val targetShape = member.targetOrSelf(codegenContext.model)
        return member.getTrait<software.amazon.smithy.model.traits.DefaultTrait>()?.toNode()?.let { node ->
            when {
                targetShape.isEnumShape && node.isStringNode -> {
                    val enumShape = targetShape.asEnumShape().get()
                    val enumSymbol = codegenContext.symbolProvider.toSymbol(targetShape)
                    val enumValue = node.expectStringNode().value
                    val enumMember =
                        enumShape.members().find { enumMember ->
                            enumMember.getTrait<software.amazon.smithy.model.traits.EnumValueTrait>()?.stringValue?.orElse(
                                enumMember.memberName,
                            ) == enumValue
                        }
                    val variantName = enumMember?.let { codegenContext.symbolProvider.toMemberName(it) } ?: enumValue
                    "$enumSymbol::$variantName"
                }

                node.isStringNode -> """"${node.expectStringNode().value}".to_string()"""
                node.isBooleanNode -> node.expectBooleanNode().value.toString()
                node.isNumberNode -> node.expectNumberNode().value.toString()
                else -> "Default::default()"
            }
        } ?: "Default::default()"
    }
}
