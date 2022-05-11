/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.testutil

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import org.junit.jupiter.params.support.AnnotationConsumer
import software.amazon.smithy.rust.codegen.smithy.CodegenMode
import java.util.stream.Stream

@Target(AnnotationTarget.TYPE, AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.RUNTIME)
@ArgumentsSource(CodegenModeProvider::class)
annotation class CodegenModeSource

class CodegenModeProvider : ArgumentsProvider, AnnotationConsumer<CodegenModeSource> {
    private lateinit var mode: CodegenModeSource

    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> {
        return listOf(CodegenMode.Client)
            .map { Arguments.of(it) }
            .stream()
    }

    override fun accept(t: CodegenModeSource) {
        this.mode = t
    }
}
