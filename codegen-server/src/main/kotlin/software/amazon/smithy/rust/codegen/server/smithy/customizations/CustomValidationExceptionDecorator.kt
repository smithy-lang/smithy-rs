package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.EnumValueTrait
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
import software.amazon.smithy.rust.codegen.server.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationFieldListTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationFieldMessageTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationFieldNameTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationMessageTrait

class CustomValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "CustomValidationExceptionDecorator"
    override val order: Byte
        get() = 69

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? {
        val res = CustomValidationExceptionConversionGenerator(codegenContext)
        res.customValidationException() ?: return null
        return res
    }
}

class CustomValidationExceptionConversionGenerator(private val codegenContext: ServerCodegenContext) :
    ValidationExceptionConversionGenerator {
    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#ValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID

    fun customValidationException(): StructureShape? {
        return codegenContext.model.shapes(StructureShape::class.java)
            .filter { it.hasTrait(ValidationExceptionTrait.ID) }
            .findFirst()
            .orElse(null)
    }

    fun customValidationMessage(): MemberShape? {
        val validationExceptionTraitShape = customValidationException() ?: return null

        return validationExceptionTraitShape.members().firstOrNull { it.hasTrait(ValidationMessageTrait.ID) }
            ?: throw CodegenException("Expected $validationExceptionTraitShape to contain a member with ValidationMessageTrait")
    }

    fun customValidationFieldList(): MemberShape? {
        val validationExceptionTraitShape = customValidationException() ?: return null

        return validationExceptionTraitShape
            .members()
            .firstOrNull { it.hasTrait(ValidationFieldListTrait.ID) }
    }

    fun customValidationField(): StructureShape? {
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

    fun customValidationFieldMessage(): MemberShape? {
        val customValidationField = customValidationField() ?: return null

        return customValidationField.members().firstOrNull { it.hasTrait(ValidationFieldMessageTrait.ID) }
    }

    fun customValidationAdditionalFields(): List<MemberShape> {
        val customValidationException = customValidationException() ?: return emptyList()
        val customValidationMessage = customValidationMessage()
        val customValidationFieldList = customValidationFieldList()

        return customValidationException.members().filter { member ->
            member != customValidationMessage &&
                member != customValidationFieldList &&
                !member.hasTrait(ValidationMessageTrait.ID) &&
                !member.hasTrait(ValidationFieldListTrait.ID)
        }
    }

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable {
        val customValidationException = customValidationException() ?: return writable { }
        val customValidationMessage = customValidationMessage() ?: return writable { }
        val customValidationFieldList = customValidationFieldList()
        val customValidationFieldMessage = customValidationFieldMessage()
        val additionalFields = customValidationAdditionalFields()

        return writable {
            val messageFormat = when {
                customValidationFieldList != null && customValidationFieldMessage != null ->
                    "format!(\"1 validation error detected. {}\", &first_validation_exception_field.#{CustomValidationFieldMessage})"

                else -> "format!(\"1 validation error detected\")"
            }

            val fieldListAssignment = when (customValidationFieldList) {
                null -> ""
                else -> "#{CustomValidationFieldList}: Some(vec![first_validation_exception_field]),"
            }

            val fieldCreation = when (customValidationFieldList) {
                null -> ""
                else -> "let first_validation_exception_field = constraint_violation.as_validation_exception_field(\"\".to_owned());"
            }

            val additionalFieldAssignments = additionalFields.joinToString("\n                            ") { member ->
                val memberName = codegenContext.symbolProvider.toMemberName(member)
                val targetShape = member.targetOrSelf(codegenContext.model)
                val defaultValue =
                    member.getTrait<software.amazon.smithy.model.traits.DefaultTrait>()?.toNode()?.let { node ->
                        when {
                            targetShape.isEnumShape && node.isStringNode -> {
                                val enumShape = targetShape.asEnumShape().get()
                                val enumSymbol = codegenContext.symbolProvider.toSymbol(targetShape)
                                val enumValue = node.expectStringNode().value
                                // Find enum member by its value, not by member name
                                val enumMember = enumShape.members().find { enumMember ->
                                    enumMember.getTrait<software.amazon.smithy.model.traits.EnumValueTrait>()?.stringValue?.orElse(
                                        enumMember.memberName
                                    ) == enumValue
                                }
                                val variantName =
                                    enumMember?.let { codegenContext.symbolProvider.toMemberName(it) } ?: enumValue
                                "crate::model::${enumSymbol.name}::$variantName"
                            }

                            node.isStringNode -> "\"${node.expectStringNode().value}\".to_string()"
                            node.isBooleanNode -> node.expectBooleanNode().value.toString()
                            node.isNumberNode -> node.expectNumberNode().value.toString()
                            else -> "Default::default()"
                        }
                    } ?: "Default::default()"
                "$memberName: $defaultValue,"
            }

            // Generate the correct shape module name for the custom validation exception
            val shapeModuleName =
                codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, customValidationException)
            val shapeFunctionName = customValidationException.id.name.toSnakeCase()

            val templateParams = mutableMapOf<String, Any>(
                "RequestRejection" to protocol.requestRejection(codegenContext.runtimeConfig),
                "CustomValidationException" to writable {
                    rust(codegenContext.symbolProvider.toSymbol(customValidationException).name)
                },
                "CustomValidationMessage" to writable {
                    rust(codegenContext.symbolProvider.toMemberName(customValidationMessage))
                },
                "From" to RuntimeType.From,
            )

            customValidationFieldList?.let {
                templateParams["CustomValidationFieldList"] = writable {
                    rust(codegenContext.symbolProvider.toMemberName(it))
                }
            }

            customValidationFieldMessage?.let {
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
                *templateParams.toList().toTypedArray()
            )
        }
    }

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable {
        val customValidationExceptionField = customValidationField() ?: return writable { }

        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to writable {
                    rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                },
                "ValidationExceptionFields" to writable {
                    stringConstraintsInfo.forEach { stringTraitInfo ->
                        when (stringTraitInfo::class.simpleName) {
                            "Length" -> {
                                val lengthTrait = stringTraitInfo::class.java.getDeclaredField("lengthTrait").apply { isAccessible = true }.get(stringTraitInfo) as LengthTrait
                                rustTemplate(
                                    """
                                    Self::Length(length) => crate::model::#{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to writable { rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name) },
                                    "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${lengthTrait.validationErrorMessage()}\", length, &path)")
                                )
                            }
                            "Pattern" -> {
                                val patternTrait = stringTraitInfo::class.java.getDeclaredField("patternTrait").apply { isAccessible = true }.get(stringTraitInfo) as PatternTrait
                                rustTemplate(
                                    """
                                    Self::Pattern(_) => crate::model::#{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to writable { rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name) },
                                    "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"Value at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: {}\", &path, \"${patternTrait.pattern}\")")
                                )
                            }
                        }
                    }
                },
            )
        }
    }

    override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable {
        val customValidationExceptionField = customValidationField() ?: return writable { }

        return writable {
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to writable {
                    rust(
                        codegenContext.symbolProvider.toSymbol(
                            customValidationExceptionField,
                        ).name,
                    )
                },
                "ValidationExceptionFields" to writable {
                    val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
                    blobConstraintsInfo.forEach { blobLength ->
                        rustTemplate(
                            """
                            Self::Length(length) => crate::model::#{CustomValidationExceptionField} {
                                #{FieldAssignments:W}
                            },
                            """,
                            "CustomValidationExceptionField" to writable {
                                rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                            },
                            "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${blobLength.lengthTrait.validationErrorMessage()}\", length, &path)")
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
        val customValidationExceptionField = customValidationField() ?: return writable { }

        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to writable {
                    rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                },
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    shape.getTrait<LengthTrait>()?.also {
                        rustTemplate(
                            """
                            Self::Length(length) => crate::model::#{CustomValidationExceptionField} {
                                #{FieldAssignments:W}
                            },""",
                            "CustomValidationExceptionField" to writable {
                                rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                            },
                            "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${it.validationErrorMessage()}\", length, &path)")
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
        val customValidationExceptionField = customValidationField() ?: return writable { }
        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            val message = enumTrait.validationErrorMessage()
            
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField} {
                    crate::model::#{CustomValidationExceptionField} {
                        #{FieldAssignments:W}
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to writable {
                    rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                },
                "FieldAssignments" to fieldAssignments("path.clone()", "format!(r##\"$message\"##, &path)")
            )
        }
    }

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable {
        val customValidationExceptionField = customValidationField() ?: return writable { }
        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField} {
                    match self {
                        Self::Range(_) => crate::model::#{CustomValidationExceptionField} {
                            #{FieldAssignments:W}
                        },
                    }
                }
                """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to writable {
                    rust(
                        codegenContext.symbolProvider.toSymbol(
                            customValidationExceptionField,
                        ).name,
                    )
                },
                "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${rangeInfo.rangeTrait.validationErrorMessage()}\", &path)")
            )
        }
    }


    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable {
        val customValidationExceptionField = customValidationField() ?: return writable { }
        
        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to writable {
                    rust(
                        codegenContext.symbolProvider.toSymbol(
                            customValidationExceptionField,
                        ).name,
                    )
                },
                "String" to RuntimeType.String,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rustTemplate(
                                """
                                ConstraintViolation::${it.name()} => crate::model::#{CustomValidationExceptionField} {
                                    #{FieldAssignments:W}
                                },
                                """.trimIndent(),
                                "CustomValidationExceptionField" to writable {
                                    rust(
                                        codegenContext.symbolProvider.toSymbol(
                                            customValidationExceptionField,
                                        ).name,
                                    )
                                },
                                "FieldAssignments" to fieldAssignments("path.clone() + \"/${it.forMember.memberName}\"", "format!(\"Value at '{}/${it.forMember.memberName}' failed to satisfy constraint: Member must not be null\", path)")
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
        val customValidationExceptionField = customValidationField() ?: return writable { }
        return writable {
            val fieldAssignments = generateCustomValidationFieldAssignments(customValidationExceptionField)
            
            rustTemplate(
                """
            pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField} {
                match self {
                    #{ValidationExceptionFields:W}
                }
            }
            """,
                "String" to RuntimeType.String,
                "CustomValidationExceptionField" to writable {
                    rust(
                        codegenContext.symbolProvider.toSymbol(
                            customValidationExceptionField,
                        ).name,
                    )
                },
                "ValidationExceptionFields" to writable {
                    collectionConstraintsInfo.forEach { collectionTraitInfo ->
                        when (collectionTraitInfo) {
                            is CollectionTraitInfo.Length -> {
                                rustTemplate(
                                    """
                                    Self::Length(length) => crate::model::#{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to writable {
                                        rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                                    },
                                    "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${collectionTraitInfo.lengthTrait.validationErrorMessage()}\", length, &path)")
                                )
                            }
                            is CollectionTraitInfo.UniqueItems -> {
                                rustTemplate(
                                    """
                                    Self::UniqueItems { duplicate_indices, .. } => crate::model::#{CustomValidationExceptionField} {
                                        #{FieldAssignments:W}
                                    },
                                    """,
                                    "CustomValidationExceptionField" to writable {
                                        rust(codegenContext.symbolProvider.toSymbol(customValidationExceptionField).name)
                                    },
                                    "FieldAssignments" to fieldAssignments("path.clone()", "format!(\"${collectionTraitInfo.uniqueItemsTrait.validationErrorMessage()}\", &duplicate_indices, &path)")
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
        val customValidationExceptionField = customValidationField() ?: return writable { }
        return writable {
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::#{CustomValidationExceptionField}",
                "CustomValidationExceptionField" to writable {
                    rust(
                        codegenContext.symbolProvider.toSymbol(
                            customValidationExceptionField,
                        ).name,
                    )
                },
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
        return { pathExpression: String, messageExpression: String ->
            writable {
                rustTemplate(
                    customValidationExceptionField.members().joinToString(",\n                ") { member ->
                        val memberName = codegenContext.symbolProvider.toMemberName(member)
                        when {
                            member.hasTrait(ValidationFieldNameTrait.ID) -> 
                                "$memberName: $pathExpression"
                            member.hasTrait(ValidationFieldMessageTrait.ID) -> 
                                "$memberName: $messageExpression"
                            else -> {
                                val targetShape = member.targetOrSelf(codegenContext.model)
                                val defaultValue = member.getTrait<software.amazon.smithy.model.traits.DefaultTrait>()?.toNode()?.let { node ->
                                    when {
                                        targetShape.isEnumShape && node.isStringNode -> {
                                            val enumShape = targetShape.asEnumShape().get()
                                            val enumSymbol = codegenContext.symbolProvider.toSymbol(targetShape)
                                            val enumValue = node.expectStringNode().value
                                            val enumMember = enumShape.members().find { enumMember ->
                                                enumMember.getTrait<software.amazon.smithy.model.traits.EnumValueTrait>()?.stringValue?.orElse(enumMember.memberName) == enumValue
                                            }
                                            val variantName = enumMember?.let { codegenContext.symbolProvider.toMemberName(it) } ?: enumValue
                                            "crate::model::${enumSymbol.name}::$variantName"
                                        }
                                        node.isStringNode -> "\"${node.expectStringNode().value}\".to_string()"
                                        node.isBooleanNode -> node.expectBooleanNode().value.toString()
                                        node.isNumberNode -> node.expectNumberNode().value.toString()
                                        else -> "Default::default()"
                                    }
                                } ?: "Default::default()"
                                "$memberName: $defaultValue"
                            }
                        }
                    }
                )
            }
        }
    }
}
