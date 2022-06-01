/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.util.hasTrait

// TODO Docs
class UnconstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    private val unconstrainedModuleWriter: RustWriter,
    private val modelsModuleWriter: RustWriter,
    val shape: MapShape
) {
    private val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
    private val name = symbol.name
    private val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
    private val constraintViolationName = constraintViolationSymbol.name
    private val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
    private val valueShape = model.expectShape(shape.value.target)
    private val constrainedSymbol = if (shape.isDirectlyConstrained(symbolProvider)) {
        symbolProvider.toSymbol(shape)
    } else {
        pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
    }

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val keySymbol = unconstrainedShapeSymbolProvider.toSymbol(keyShape)
        val valueSymbol = unconstrainedShapeSymbolProvider.toSymbol(valueShape)

        unconstrainedModuleWriter.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);
                
                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                
                """,
                "KeySymbol" to keySymbol,
                "ValueSymbol" to valueSymbol,
                "MaybeConstrained" to constrainedSymbol.makeMaybeConstrained(),
            )

            renderTryFromUnconstrainedForConstrained(this)
        }

        renderConstraintViolation()
    }

    private fun renderConstraintViolation() {
        val constraintViolationCodegenScope = listOfNotNull(
            if (isKeyConstrained(keyShape)) {
                "KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape)
            } else {
                null
            },
            if (isValueConstrained(valueShape)) {
                "ValueConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(valueShape)
            } else {
                null
            },
        ).toTypedArray()

        modelsModuleWriter.withModule(
            constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last()
        ) {
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub enum $constraintViolationName {
                    ${if (shape.hasTrait<LengthTrait>()) "Length(usize)," else ""}
                    ${if (isKeyConstrained(keyShape)) "##[doc(hidden)] Key(#{KeyConstraintViolationSymbol})," else ""}
                    ${if (isValueConstrained(valueShape)) "##[doc(hidden)] Value(#{ValueConstraintViolationSymbol})," else ""}
                }
                """,
                *constraintViolationCodegenScope,
            )
        }
    }

    private fun renderTryFromUnconstrainedForConstrained(writer: RustWriter) {
        writer.rustBlock("impl std::convert::TryFrom<$name> for #{T}", constrainedSymbol) {
            rust("type Error = #T;", constraintViolationSymbol)

            rustBlock("fn try_from(value: $name) -> Result<Self, Self::Error>") {
                if (isKeyConstrained(keyShape) || isValueConstrained(valueShape)) {
                    val resolveToNonPublicConstrainedValueType =
                        isValueConstrained(valueShape) &&
                                !valueShape.isDirectlyConstrained(symbolProvider) &&
                                !valueShape.isStructureShape
                    val constrainedValueSymbol = if (resolveToNonPublicConstrainedValueType) {
                        pubCrateConstrainedShapeSymbolProvider.toSymbol(valueShape)
                    } else {
                        symbolProvider.toSymbol(valueShape)
                    }

                    rustTemplate(
                        """
                        let res: Result<std::collections::HashMap<#{KeySymbol}, #{ConstrainedValueSymbol}>, Self::Error> = value.0
                            .into_iter()
                            .map(|(k, v)| {
                                ${if (isKeyConstrained(keyShape)) "let k = k.try_into().map_err(Self::Error::Key)?;" else ""}
                                ${if (isValueConstrained(valueShape)) "let v = v.try_into().map_err(Self::Error::Value)?;" else ""}
                                Ok((k, v))
                            })
                            .collect();
                        let hm = res?;
                        """,
                        "KeySymbol" to symbolProvider.toSymbol(keyShape),
                        "ConstrainedValueSymbol" to constrainedValueSymbol
                    )

                    val constrainedValueTypeIsNotFinalType =
                        resolveToNonPublicConstrainedValueType && shape.isDirectlyConstrained(symbolProvider)
                    if (constrainedValueTypeIsNotFinalType) {
                        // The map is constrained. Its value shape reaches a constrained shape, but the value shape itself
                        // is not directly constrained. The value shape must be an aggregate shape. But it is not a
                        // structure shape. So it must be a collection or map shape. In this case the type for the value
                        // shape that implements the `Constrained` trait _does not_ coincide with the regular type the user
                        // is exposed to. The former will be the `pub(crate)` wrapper tuple type created by a
                        // `Constrained*Generator`, whereas the latter will be an stdlib container type. Both types are
                        // isomorphic though, and we can convert between them using `From`, so that's what we do here.
                        //
                        // As a concrete example of this particular case, consider the model:
                        //
                        // ```smithy
                        // @length(min: 1)
                        // map Map {
                        //    key: String,
                        //    value: List,
                        // }
                        //
                        // list List {
                        //     member: NiceString
                        // }
                        //
                        // @length(min: 1, max: 69)
                        // string NiceString
                        // ```
                        rustTemplate(
                            """
                            let hm: std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}> = 
                                hm.into_iter().map(|(k, v)| (k, v.into())).collect();
                            """,
                            "KeySymbol" to symbolProvider.toSymbol(keyShape),
                            "ValueSymbol" to symbolProvider.toSymbol(valueShape)
                        )
                    }
                } else {
                    rust("let hm = value.0;")
                }

                if (shape.isDirectlyConstrained(symbolProvider)) {
                    rust("Self::try_from(hm)")
                } else {
                    rust("Ok(Self(hm))")
                }
            }
        }
    }

    private fun isKeyConstrained(shape: StringShape) = shape.isDirectlyConstrained(symbolProvider)

    private fun isValueConstrained(shape: Shape): Boolean = shape.canReachConstrainedShape(model, symbolProvider)
}
