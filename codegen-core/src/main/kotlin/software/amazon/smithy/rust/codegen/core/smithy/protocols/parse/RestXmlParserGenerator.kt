/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.traits.AllowInvalidXmlRoot
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull

class RestXmlParserGenerator(
    codegenContext: CodegenContext,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            codegenContext,
            xmlErrors,
        ) { context, inner ->
            val shapeName = context.outputShapeName
            // Get the non-synthetic version of the outputShape and check to see if it has the `AllowInvalidXmlRoot` trait
            val allowInvalidRoot =
                context.model.getShape(context.shape.outputShape).orNull().let { shape ->
                    shape?.getTrait<SyntheticOutputTrait>()?.originalId.let { shapeId ->
                        context.model.getShape(shapeId).orNull()?.hasTrait<AllowInvalidXmlRoot>() ?: false
                    }
                }

            // If we DON'T allow the XML root to be invalid, insert code to check for and report a mismatch
            if (!allowInvalidRoot) {
                rustTemplate(
                    """
                    if !${XmlBindingTraitParserGenerator.XmlName(shapeName).matchExpression("start_el")} {
                        return Err(
                            #{XmlDecodeError}::custom(
                                format!("encountered invalid XML root: expected $shapeName but got {start_el:?}. This is likely a bug in the SDK.")
                            )
                        )
                    }
                    """,
                    "XmlDecodeError" to context.xmlDecodeErrorType,
                )
            }

            inner("decoder")
        },
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
