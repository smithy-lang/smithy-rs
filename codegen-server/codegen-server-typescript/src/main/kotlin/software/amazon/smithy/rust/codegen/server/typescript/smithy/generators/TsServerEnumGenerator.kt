/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGeneratorContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedEnum
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * To share enums defined in Rust with Typescript, `napi-rs` provides the `napi` trait.
 * This class generates enums definitions and implements the `napi` trait.
 */
class TsConstrainedEnum(
    codegenContext: ServerCodegenContext,
    shape: StringShape,
    validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) : ConstrainedEnum(codegenContext, shape, validationExceptionConversionGenerator) {
    private val napiDerive = TsServerCargoDependency.NapiDerive.toType()

    override fun additionalEnumImpls(context: EnumGeneratorContext): Writable =
        writable {
            this.rust("use napi::bindgen_prelude::ToNapiValue;")
        }

    override fun additionalEnumAttributes(context: EnumGeneratorContext): List<Attribute> =
        listOf(Attribute(napiDerive.resolve("napi")))
}

class TsServerEnumGenerator(
    codegenContext: ServerCodegenContext,
    shape: StringShape,
    validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
    customizations: List<EnumCustomization>,
) : EnumGenerator(
        codegenContext.model,
        codegenContext.symbolProvider,
        shape,
        TsConstrainedEnum(
            codegenContext,
            shape,
            validationExceptionConversionGenerator,
        ),
        customizations,
    )
