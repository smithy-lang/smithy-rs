package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.traits.ValidationExceptionTrait

class CustomValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "CustomValidationExceptionDecorator"
    override val order: Byte
        get() = 69

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator? {
        codegenContext.customExceptionName ?: return null
        return CustomValidationExceptionConversionGenerator(codegenContext)
    }
}

class CustomValidationExceptionConversionGenerator(private val codegenContext: ServerCodegenContext) :
    ValidationExceptionConversionGenerator {
    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#ValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID
    private val fieldGenerator = ValidationExceptionDecoratorGenerator(codegenContext)

//    fun generateCustomValidationExceptionBuilder(): Writable = writable {
//        val customExceptionName = codegenContext.customExceptionName ?: return@writable
//
//        rustTemplate(
//            """
//            impl #{CustomExceptionName} {
//                /// Create a new builder for the custom validation exception
//                pub fn builder() -> #{CustomExceptionName}Builder {
//                    #{CustomExceptionName}Builder::default()
//                }
//            }
//
//            /// Builder for #{CustomExceptionName}
//            #[derive(Default)]
//            pub struct #{CustomExceptionName}Builder {
//                message: Option<String>,
//                field_list: Option<Vec<ValidationExceptionField>>,
//                // Add additional fields from the custom exception model
//                #{AdditionalBuilderFields}
//            }
//
//            impl #{CustomExceptionName}Builder {
//                /// Set the error message
//                pub fn message(mut self, message: impl Into<String>) -> Self {
//                    self.message = Some(message.into());
//                    self
//                }
//
//                /// Set the list of validation exception fields
//                pub fn field_list(mut self, field_list: Vec<ValidationExceptionField>) -> Self {
//                    self.field_list = Some(field_list);
//                    self
//                }
//
//                #{AdditionalBuilderMethods}
//
//                /// Build the custom validation exception
//                pub fn build(self) -> Result<#{CustomExceptionName}, String> {
//                    let message = self.message.ok_or("message is required")?;
//
//                    Ok(#{CustomExceptionName} {
//                        message,
//                        field_list: self.field_list,
//                        #{AdditionalBuildFields}
//                    })
//                }
//            }
//            """,
//            "CustomExceptionName" to customExceptionName,
//            "AdditionalBuilderFields" to renderAdditionalBuilderFields(),
//            "AdditionalBuilderMethods" to renderAdditionalBuilderMethods(),
//            "AdditionalBuildFields" to renderAdditionalBuildFields(),
//        )
//    }

    internal fun getCustomExceptionShape(): StructureShape? =
        codegenContext.model.shapes(StructureShape::class.java)
            .filter { it.hasTrait(ValidationExceptionTrait.ID) }
            .findFirst()
            .orElse(null)

    internal fun getAdditionalMembers(): List<MemberShape> =
        getCustomExceptionShape()?.members()
            ?.filter { !it.memberName.equals("message") && !it.memberName.equals("fieldList") }
            ?: emptyList()

//    private fun renderAdditionalBuilderFields(): String =
//        getAdditionalMembers().map { member ->
//            val memberSymbol = codegenContext.symbolProvider.toSymbol(member)
//            "${member.memberName}: Option<${memberSymbol.rustType()}>,"
//        }.joinToString("\n                ")
//
//    private fun renderAdditionalBuilderMethods(): String =
//        getAdditionalMembers().map { member ->
//            val memberSymbol = codegenContext.symbolProvider.toSymbol(member)
//            """
//            /// Set the ${member.memberName}
//            pub fn ${member.memberName}(mut self, ${member.memberName}: impl Into<${memberSymbol.rustType()}>) -> Self {
//                self.${member.memberName} = Some(${member.memberName}.into());
//                self
//            }
//            """.trimIndent()
//        }.joinToString("\n                \n                ")
//
//    private fun renderAdditionalBuildFields(): String =
//        getAdditionalMembers().map { member ->
//            "${member.memberName}: self.${member.memberName},"
//        }.joinToString("\n                        ")

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
        fieldGenerator.renderImplFromConstraintViolationForRequestRejection(protocol)

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable =
        fieldGenerator.stringShapeConstraintViolationImplBlock(stringConstraintsInfo)

    override fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait): Writable =
        fieldGenerator.enumShapeConstraintViolationImplBlock(enumTrait)

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable =
        fieldGenerator.numberShapeConstraintViolationImplBlock(rangeInfo)

    override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable =
        fieldGenerator.blobShapeConstraintViolationImplBlock(blobConstraintsInfo)

    override fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ): Writable =
        fieldGenerator.mapShapeConstraintViolationImplBlock(shape, keyShape, valueShape, symbolProvider, model)

    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable =
        fieldGenerator.builderConstraintViolationFn(constraintViolations)

    override fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ): Writable =
        fieldGenerator.collectionShapeConstraintViolationImplBlock(collectionConstraintsInfo, isMemberConstrained)

    override fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ): Writable = fieldGenerator.unionShapeConstraintViolationImplBlock(unionConstraintTraitInfo)
}
