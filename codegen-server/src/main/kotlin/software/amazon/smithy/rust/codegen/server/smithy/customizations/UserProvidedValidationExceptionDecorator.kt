/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.framework.rust.ValidationExceptionTrait
import software.amazon.smithy.framework.rust.ValidationFieldListTrait
import software.amazon.smithy.framework.rust.ValidationFieldMessageTrait
import software.amazon.smithy.framework.rust.ValidationFieldNameTrait
import software.amazon.smithy.framework.rust.ValidationMessageTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
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
import software.amazon.smithy.rust.codegen.core.util.targetShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
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
import software.amazon.smithy.rust.codegen.server.smithy.util.isValidationFieldName
import software.amazon.smithy.rust.codegen.server.smithy.util.isValidationMessage
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

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

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? =
        firstStructureShapeWithValidationExceptionTrait(codegenContext.model)?.let {
            UserProvidedValidationExceptionConversionGenerator(
                codegenContext,
                it,
                validationMessageMember(it),
                maybeValidationFieldList(codegenContext.model, it),
                additionalFieldMembers(it),
            )
        }

    internal fun firstStructureShapeWithValidationExceptionTrait(model: Model): StructureShape? =
        model
            .shapes(StructureShape::class.java)
            .toList()
            // Defining multiple validation exceptions is unsupported. See `ValidateUnsupportedConstraints`
            .firstOrNull({ it.hasTrait(ValidationExceptionTrait.ID) })

    internal fun validationMessageMember(validationExceptionStructure: StructureShape): MemberShape =
        validationExceptionStructure
            .members()
            .firstOrNull { it.isValidationMessage() }
            ?: throw CodegenException("Expected `$validationExceptionStructure` to contain a member named `message` or annotated with the `@validationMessageTrait`")

    internal fun additionalFieldMembers(validationExceptionStructure: StructureShape): List<MemberShape> =
        validationExceptionStructure.members().filter { member ->
            !member.isValidationMessage() &&
                !member.hasTrait(
                    ValidationFieldListTrait.ID,
                )
        }

    /**
     * Returns a [ValidationFieldList] if the following exist:
     * - A structure type representing the field
     * - A list type representing the field list with a single member targeting the field type
     * - A member in the validation exception structure annotated with `@validationFieldList` targeting the list type
     *
     * Returns null if there is no member annotated with the `@validationFieldList` trait in the given validation exception structure
     * Otherwise, throws a [CodegenException] if it exists, but is misconfigured
     */
    internal fun maybeValidationFieldList(
        model: Model,
        validationExceptionStructure: StructureShape,
    ): ValidationFieldList? {
        val validationFieldListMember =
            validationExceptionStructure
                .members()
                .firstOrNull { it.hasTrait(ValidationFieldListTrait.ID) }
                ?: return null

        val validationFieldListShape =
            validationFieldListMember
                .targetShape(model)
                .asListShape()
                .orElseThrow {
                    CodegenException("Expected `$validationFieldListMember` to target a list type")
                }

        val validationFieldListShapeMember =
            validationFieldListShape.members().singleOrNull()
                ?: throw CodegenException("Expected `$validationFieldListShape` to have a single member")

        val validationFieldStructure =
            validationFieldListShapeMember
                .targetShape(model)
                .asStructureShape()
                .orElseThrow {
                    CodegenException("Expected $validationFieldListShapeMember to target a structure type")
                }

        // It is required that a member of the user provided validation field structure has @validationFieldName
        val validationFieldNameMember =
            validationFieldStructure
                .members()
                .firstOrNull { it.isValidationFieldName() }
                ?: throw CodegenException("Expected `$validationFieldStructure` to contain a member with the `@validationFieldName` trait")

        val maybeValidationFieldMessageMember =
            validationFieldStructure
                .members()
                .firstOrNull { it.hasTrait(ValidationFieldMessageTrait.ID) }

        return ValidationFieldList(
            validationFieldListMember,
            validationFieldStructure,
            validationFieldNameMember,
            maybeValidationFieldMessageMember,
        )
    }

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ServerRustSettings,
    ): Model {
        val validationExceptionStructure = firstStructureShapeWithValidationExceptionTrait(model) ?: return model
        annotateValidationMessageMember(validationExceptionStructure)
        maybeValidationFieldList(model, validationExceptionStructure)?.let {
            annotateValidationFieldName(it)
        }

        return model
    }

    /**
     * Annotates the "message" member of the validation exception structure with @validationMessage when there is no
     * explicitly annotated member
     */
    internal fun annotateValidationMessageMember(validationExceptionStructure: StructureShape) {
        val member = validationMessageMember(validationExceptionStructure)
        if (!member.hasTrait(ValidationMessageTrait.ID)) {
            // When there is no field annotated with the @validationMessage trait, we will annotate the field named "message"
            member.toBuilder().addTrait(ValidationMessageTrait(SourceLocation.none()))
        }
    }

    /**
     * Annotates the "name" member of the validation field structure with @validationFieldName when there is no
     * explicitly annotated member
     */
    internal fun annotateValidationFieldName(validationFieldList: ValidationFieldList) {
        val member = validationFieldList.validationFieldNameMember
        if (!member.hasTrait(ValidationFieldNameTrait.ID)) {
            // When there is no field annotated with the @validationMessage trait, we will annotate the field named "name"
            member.toBuilder().addTrait(ValidationFieldNameTrait(SourceLocation.none()))
        }
    }
}

class UserProvidedValidationExceptionConversionGenerator(
    private val codegenContext: ServerCodegenContext,
    private val validationExceptionStructure: StructureShape,
    private val validationMessageMember: MemberShape,
    private val maybeValidationFieldList: ValidationFieldList?,
    private val additionalFieldMembers: List<MemberShape>,
) : ValidationExceptionConversionGenerator {
    private val codegenScope =
        listOfNotNull(maybeValidationFieldList?.validationFieldStructure)
            .map {
                "ValidationExceptionField" to codegenContext.symbolProvider.toSymbol(it)
            }.toTypedArray()

    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#UserProvidedValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
        writable {
            val validationMessageName = codegenContext.symbolProvider.toMemberName(validationMessageMember)
            // Generate the correct shape module name for the user provided validation exception
            val shapeModuleName =
                codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, validationExceptionStructure)
            val shapeFunctionName = validationExceptionStructure.id.name.toSnakeCase()

            rustTemplate(
                """
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        #{FieldCreation}
                        let validation_exception = #{ValidationException} {
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
                "ValidationException" to codegenContext.symbolProvider.toSymbol(validationExceptionStructure),
                "FieldCreation" to
                    writable {
                        if (maybeValidationFieldList?.maybeValidationFieldMessageMember != null) {
                            rust("""let first_validation_exception_field = constraint_violation.as_validation_exception_field("".to_owned());""")
                        }
                    },
                "ValidationMessage" to
                    writable {
                        val message =
                            maybeValidationFieldList?.maybeValidationFieldMessageMember?.let {
                                val validationFieldMessageName =
                                    codegenContext.symbolProvider.toMemberName(it)
                                if (it.isOptional) {
                                    """format!("validation error detected. {}", &first_validation_exception_field.$validationFieldMessageName.clone().unwrap_or_default())"""
                                } else {
                                    """format!("validation error detected. {}", &first_validation_exception_field.$validationFieldMessageName)"""
                                }
                            } ?: """format!("validation error detected")"""
                        rust(validationMessageMember.wrapValueIfOptional(message))
                    },
                "FieldListAssignment" to
                    writable {
                        maybeValidationFieldList?.validationFieldListMember?.let {
                            val fieldName =
                                codegenContext.symbolProvider.toMemberName(it)
                            val value = it.wrapValueIfOptional("vec![first_validation_exception_field]")
                            rust("$fieldName: $value,")
                        }
                    },
                "AdditionalFieldAssignments" to
                    writable {
                        rust(
                            additionalFieldMembers.joinToString { member ->
                                val memberName = codegenContext.symbolProvider.toMemberName(member)!!
                                "$memberName: ${defaultFieldAssignment(member)}"
                            },
                        )
                    },
            )
        }

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable {
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenScope,
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
                                        Self::Length(length) => #{ValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenScope,
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
                                        Self::Pattern(_) => #{ValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenScope,
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
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenScope,
                "ValidationExceptionFields" to
                    writable {
                        val fieldAssignments =
                            generateUserProvidedValidationFieldAssignments(validationFieldList)
                        blobConstraintsInfo.forEach { blobLength ->
                            rustTemplate(
                                """
                                Self::Length(length) => #{ValidationExceptionField} {
                                    #{FieldAssignments}
                                },
                                """,
                                *codegenScope,
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
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField}",
                *preludeScope,
                *codegenScope,
            ) {
                rustBlock("match self") {
                    shape.getTrait<LengthTrait>()?.also {
                        rustTemplate(
                            """
                            Self::Length(length) => #{ValidationExceptionField} {
                                #{FieldAssignments}
                            },""",
                            *codegenScope,
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
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)
            val message = enumTrait.validationErrorMessage()

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField} {
                    #{ValidationExceptionField} {
                        #{FieldAssignments}
                    }
                }
                """,
                *preludeScope,
                *codegenScope,
                "FieldAssignments" to fieldAssignments("path.clone()", """format!(r##"$message"##, &path)"""),
            )
        }
    }

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable {
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField} {
                    match self {
                        Self::Range(_) => #{ValidationExceptionField} {
                            #{FieldAssignments}
                        },
                    }
                }
                """,
                *preludeScope,
                *codegenScope,
                "FieldAssignments" to
                    fieldAssignments(
                        "path.clone()",
                        """format!(${rangeInfo.rangeTrait.validationErrorMessage().dq()}, &path)""",
                    ),
            )
        }
    }

    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable {
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)

            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField}",
                *preludeScope,
                *codegenScope,
            ) {
                rustBlock("match self") {
                    constraintViolations.forEach {
                        if (it.hasInner()) {
                            rust("""ConstraintViolation::${it.name()}(inner) => inner.as_validation_exception_field(path + "/${it.forMember.memberName}"),""")
                        } else {
                            rustTemplate(
                                """
                                ConstraintViolation::${it.name()} => #{ValidationExceptionField} {
                                    #{FieldAssignments}
                                },
                                """.trimIndent(),
                                *codegenScope,
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
        val validationFieldList = maybeValidationFieldList ?: return writable { }

        return writable {
            val fieldAssignments = generateUserProvidedValidationFieldAssignments(validationFieldList)

            rustTemplate(
                """
                pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField} {
                    match self {
                        #{ValidationExceptionFields}
                    }
                }
                """,
                *preludeScope,
                *codegenScope,
                "ValidationExceptionFields" to
                    writable {
                        collectionConstraintsInfo.forEach { collectionTraitInfo ->
                            when (collectionTraitInfo) {
                                is CollectionTraitInfo.Length -> {
                                    rustTemplate(
                                        """
                                        Self::Length(length) => #{ValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenScope,
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
                                        Self::UniqueItems { duplicate_indices, .. } => #{ValidationExceptionField} {
                                            #{FieldAssignments}
                                        },
                                        """,
                                        *codegenScope,
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
        maybeValidationFieldList ?: return writable { }

        return writable {
            rustBlockTemplate(
                "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> #{ValidationExceptionField}",
                *preludeScope,
                *codegenScope,
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
        validationFieldList: ValidationFieldList,
    ): (String, String) -> Writable =
        { rawPathExpression: String, rawMessageExpression: String ->
            writable {
                rustTemplate(
                    validationFieldList.validationFieldStructure.members().joinToString(",") { member ->
                        val memberName = codegenContext.symbolProvider.toMemberName(member)
                        val pathExpression = member.wrapValueIfOptional(rawPathExpression)
                        val messageExpression = member.wrapValueIfOptional(rawMessageExpression)
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
        val targetShape = member.targetShape(codegenContext.model)
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

    private fun MemberShape.wrapValueIfOptional(valueExpression: String): String =
        if (this.isOptional) {
            "Some($valueExpression)"
        } else {
            valueExpression
        }
}

/**
 * Class for encapsulating data related to validation field list
 */
class ValidationFieldList(
    val validationFieldListMember: MemberShape,
    val validationFieldStructure: StructureShape,
    val validationFieldNameMember: MemberShape,
    val maybeValidationFieldMessageMember: MemberShape?,
)
