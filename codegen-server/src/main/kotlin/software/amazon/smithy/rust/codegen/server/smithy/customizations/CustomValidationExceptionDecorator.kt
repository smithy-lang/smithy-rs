package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
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
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage
import software.amazon.smithy.rust.codegen.server.smithy.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.ValidationFieldListTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.ValidationFieldMessageTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.ValidationFieldNameTrait
import software.amazon.smithy.rust.codegen.server.smithy.util.isValidationMessage

/**
 * Decorator for custom validation exception codegen
 *
 * The order of this is less than that of [SmithyValidationExceptionDecorator] so it takes precedence regardless of the
 * order decorators are passed into the plugin
 */
class CustomValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "CustomValidationExceptionDecorator"
    override val order: Byte
        get() = 68

    internal fun customValidationException(codegenContext: ServerCodegenContext): StructureShape? {
        return codegenContext.model.shapes(StructureShape::class.java)
            .filter { it.hasTrait(ValidationExceptionTrait.ID) }
            .findFirst()
            .orElse(null)
    }

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? {
        val validationException = customValidationException(codegenContext) ?: return null

        return CustomValidationExceptionConversionGenerator(codegenContext, validationException)
    }
}

class CustomValidationExceptionConversionGenerator(
    private val codegenContext: ServerCodegenContext,
    private val validationException: StructureShape,
) : ValidationExceptionConversionGenerator {
    private val maybeValidationField = customValidationField()?.let { field ->
        ValidationField(codegenContext, field)
    }

    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#CustomValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID

    internal fun customValidationMessage(): MemberShape {
        return validationException.members()
            .firstOrNull { it.isValidationMessage() }
            ?: throw CodegenException("Expected $validationException to contain a member with ValidationMessageTrait")
    }

    internal fun customValidationFieldList(): MemberShape? {
        return validationException
            .members()
            .firstOrNull { it.hasTrait(ValidationFieldListTrait.ID) }
    }

    internal fun customValidationField(): StructureShape? {
        val validationFieldListMember = customValidationFieldList() ?: return null

        // get target of member of the custom validation field list that represents the structure for the
        // validation field shape, otherwise, we return null as field list is optional
        val validationFieldShape = validationFieldListMember
            .targetOrSelf(codegenContext.model)
            .members()
            .firstOrNull() { it.targetOrSelf(codegenContext.model).isStructureShape }
            ?.targetOrSelf(codegenContext.model)
            ?.asStructureShape()
            ?.orElse(null)
            ?: return null

        // It is required that a member of the custom validation field structure has @validationFieldName
        if (validationFieldShape.members()
                .none { it.hasTrait(ValidationFieldNameTrait.ID) }
        ) throw CodegenException("Expected $validationFieldShape to contain a member with ValidationFieldNameTrait")

        return validationFieldShape
    }

    internal fun customValidationFieldMessage(): MemberShape? {
        val validationField = customValidationField() ?: return null

        return validationField.members().firstOrNull { it.hasTrait(ValidationFieldMessageTrait.ID) }
    }

    internal fun customValidationAdditionalFields(): List<MemberShape> {
        return validationException.members().filter { member ->
            !member.isValidationMessage() && !member.hasTrait(ValidationFieldListTrait.ID)
        }
    }

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable {
        val validationMessage = customValidationMessage()
        val validationFieldList = customValidationFieldList()
        val validationFieldMessage = customValidationFieldMessage()
        val additionalFields = customValidationAdditionalFields()

        return writable {
            var messageFormat = when {
                validationFieldList != null && validationFieldMessage != null -> {
                    if (validationFieldMessage.isOptional) {
                        """format!("validation error detected. {}", &first_validation_exception_field.#{CustomValidationFieldMessage}.clone().unwrap_or_default())"""
                    } else {
                        """format!("validation error detected. {}", &first_validation_exception_field.#{CustomValidationFieldMessage})"""
                    }
                }

                else -> """format!("validation error detected")"""
            }
            if (validationMessage.isOptional) {
                messageFormat = "Some($messageFormat)"
            }

            val fieldListAssignment = when (validationFieldList) {
                null -> ""
                else -> {
                    if (validationFieldList.isOptional) {
                        "#{CustomValidationFieldList}: Some(vec![first_validation_exception_field]),"
                    } else {
                        "#{CustomValidationFieldList}: vec![first_validation_exception_field],"
                    }
                }
            }

            val fieldCreation = when (validationFieldList) {
                null -> ""
                else -> """let first_validation_exception_field = constraint_violation.as_validation_exception_field("".to_owned());"""
            }

            val additionalFieldAssignments = additionalFields.joinToString { member ->
                val memberName = codegenContext.symbolProvider.toMemberName(member)
                "$memberName: ${defaultFieldAssignment(member)}"
            }

            // Generate the correct shape module name for the custom validation exception
            val shapeModuleName =
                codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, validationException)
            val shapeFunctionName = validationException.id.name.toSnakeCase()

            val templateParams = mutableMapOf<String, Any>(
                "RequestRejection" to protocol.requestRejection(codegenContext.runtimeConfig),
                "CustomValidationException" to writable {
                    rust(codegenContext.symbolProvider.toSymbol(validationException).name)
                },
                "CustomValidationMessage" to writable {
                    rust(codegenContext.symbolProvider.toMemberName(validationMessage))
                },
                "From" to RuntimeType.From,
            )

            validationFieldList?.let {
                templateParams["CustomValidationFieldList"] = writable {
                    rust(codegenContext.symbolProvider.toMemberName(it))
                }
            }

            validationFieldMessage?.let {
                templateParams["CustomValidationFieldMessage"] = writable {
                    rust(codegenContext.symbolProvider.toMemberName(it))
                }
            }

            rustTemplate(
                """
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        $fieldCreation
                        let validation_exception = crate::error::#{CustomValidationException} {
                            #{CustomValidationMessage}: $messageFormat,
                            $fieldListAssignment
                            $additionalFieldAssignments
                        };
                        Self::ConstraintViolation(
                            crate::protocol_serde::$shapeModuleName::ser_${shapeFunctionName}_error(&validation_exception)
                                .expect("validation exceptions should never fail to serialize; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
                        )
                    }
                }
                """,
                *templateParams.toList().toTypedArray(),
            )
        }
    }

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to validationField.writable,
                "ValidationExceptionFields" to writable {
                    stringConstraintsInfo.forEach { stringTraitInfo ->
                        when (stringTraitInfo::class.simpleName) {
                            "Length" -> {
                                val lengthTrait = stringTraitInfo::class.java.getDeclaredField("lengthTrait")
                                    .apply { isAccessible = true }.get(stringTraitInfo) as LengthTrait
                                rustTemplate(
                                    """
                                    Self::Length(length) => #{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to validationField.writable,
                                    "FieldAssignments" to fieldAssignments(
                                        "path.clone()",
                                        """format!("${lengthTrait.validationErrorMessage()}", length, &path)""",
                                    ),
                                )
                            }

                            "Pattern" -> {
                                val patternTrait = stringTraitInfo::class.java.getDeclaredField("patternTrait")
                                    .apply { isAccessible = true }.get(stringTraitInfo) as PatternTrait
                                rustTemplate(
                                    """
                                    Self::Pattern(_) => #{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to validationField.writable,
                                    "FieldAssignments" to fieldAssignments(
                                        "path.clone()",
                                        """format!("Value at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: {}", &path, "${patternTrait.pattern}")""",
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
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to validationField.writable,
                "ValidationExceptionFields" to writable {
                    val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)
                    blobConstraintsInfo.forEach { blobLength ->
                        rustTemplate(
                            """
                            Self::Length(length) => #{CustomValidationExceptionField} {
                                #{FieldAssignments:W}
                            },
                            """,
                            "CustomValidationExceptionField" to validationField.writable,
                            "FieldAssignments" to fieldAssignments(
                                "path.clone()",
                                """format!("${blobLength.lengthTrait.validationErrorMessage()}", length, &path)""",
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
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to validationField.writable,
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    shape.getTrait<LengthTrait>()?.also {
                        rustTemplate(
                            """
                            Self::Length(length) => #{CustomValidationExceptionField} {
                                #{FieldAssignments:W}
                            },""",
                            "CustomValidationExceptionField" to validationField.writable,
                            "FieldAssignments" to fieldAssignments(
                                "path.clone()",
                                """format!("${it.validationErrorMessage()}", length, &path)""",
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
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)
            val message = enumTrait.validationErrorMessage()

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField} {
                    #{CustomValidationExceptionField} {
                        #{FieldAssignments:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to validationField.writable,
                "FieldAssignments" to fieldAssignments("path.clone()", """format!(r##"$message"##, &path)"""),
            )
        }
    }

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField} {
                    match self {
                        Self::Range(_) => #{CustomValidationExceptionField} {
                            #{FieldAssignments:W}
                        },
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to validationField.writable,
                "FieldAssignments" to fieldAssignments(
                    "path.clone()",
                    """format!("${rangeInfo.rangeTrait.validationErrorMessage()}", &path)""",
                ),
            )
        }
    }


    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable {
        val validationField = maybeValidationField ?: return writable { }

        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to validationField.writable,
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rustTemplate(
                                """
                                ConstraintViolation::${it.name()} => #{CustomValidationExceptionField} {
                                    #{FieldAssignments:W}
                                },
                                """.trimIndent(),
                                "CustomValidationExceptionField" to validationField.writable,
                                "FieldAssignments" to fieldAssignments(
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
            val fieldAssignments = generateCustomValidationFieldAssignments(validationField.shape)

            rustTemplate(
                """
            pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField} {
                match self {
                    #{ValidationExceptionFields:W}
                }
            }
            """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to validationField.writable,
                "ValidationExceptionFields" to writable {
                    collectionConstraintsInfo.forEach { collectionTraitInfo ->
                        when (collectionTraitInfo) {
                            is CollectionTraitInfo.Length -> {
                                rustTemplate(
                                    """
                                    Self::Length(length) => #{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to validationField.writable,
                                    "FieldAssignments" to fieldAssignments(
                                        "path.clone()",
                                        """format!("${collectionTraitInfo.lengthTrait.validationErrorMessage()}", length, &path)""",
                                    ),
                                )
                            }

                            is CollectionTraitInfo.UniqueItems -> {
                                rustTemplate(
                                    """
                                    Self::UniqueItems { duplicate_indices, .. } => #{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to validationField.writable,
                                    "FieldAssignments" to fieldAssignments(
                                        "path.clone()",
                                        """format!("${collectionTraitInfo.uniqueItemsTrait.validationErrorMessage()}", &duplicate_indices, &path)""",
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
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to validationField.writable,
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

    /**
     * Helper function to generate field assignments for custom validation exception fields
     */
    private fun generateCustomValidationFieldAssignments(customValidationExceptionField: StructureShape): (String, String) -> Writable {
        return { rawPathExpression: String, rawMessageExpression: String ->
            writable {
                rustTemplate(
                    customValidationExceptionField.members().joinToString(",") { member ->
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
    }

    private fun defaultFieldAssignment(member: MemberShape): String {
        val targetShape = member.targetOrSelf(codegenContext.model)
        return member.getTrait<software.amazon.smithy.model.traits.DefaultTrait>()?.toNode()?.let { node ->
            when {
                targetShape.isEnumShape && node.isStringNode -> {
                    val enumShape = targetShape.asEnumShape().get()
                    val enumSymbol = codegenContext.symbolProvider.toSymbol(targetShape)
                    val enumValue = node.expectStringNode().value
                    val enumMember = enumShape.members().find { enumMember ->
                        enumMember.getTrait<software.amazon.smithy.model.traits.EnumValueTrait>()?.stringValue?.orElse(
                            enumMember.memberName,
                        ) == enumValue
                    }
                    val variantName = enumMember?.let { codegenContext.symbolProvider.toMemberName(it) } ?: enumValue
                    "crate::model::${enumSymbol.name}::$variantName"
                }

                node.isStringNode -> """"${node.expectStringNode().value}".to_string()"""
                node.isBooleanNode -> node.expectBooleanNode().value.toString()
                node.isNumberNode -> node.expectNumberNode().value.toString()
                else -> "Default::default()"
            }
        } ?: "Default::default()"
    }
}

private class ValidationField(codegenContext: ServerCodegenContext, val shape: StructureShape) {
    val writable = writable { rust(codegenContext.symbolProvider.toSymbol(shape).toString()) }
}
