/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

class RestXmlParserGenerator(
    protocolConfig: ProtocolConfig,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            protocolConfig,
            xmlErrors
        ) { context, inner ->
            val shapeName = context.outputShapeName
            rustTemplate(
                """
                if !(${XmlBindingTraitParserGenerator.XmlName(shapeName).matchExpression("start_el")}) {
                    return Err(#{XmlError}::custom(format!("invalid root, expected $shapeName got {:?}", start_el)))
                }
                """,
                "XmlError" to context.xmlErrorType
            )
            inner("decoder")
        }
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
