/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.GenericTypeArg
import software.amazon.smithy.rust.codegen.smithy.generators.GenericsGenerator
import software.amazon.smithy.rust.codegen.testutil.compileAndTest

internal class WritableTest {
    private fun testItCompiles(typeParameters: Writable, value: String) {
        val testWriter = RustWriter.root()
        testWriter.withModule("test") {
            rustTemplate("""
                use std::marker::PhantomData;
                pub struct Test<T> { _type: PhantomData<T> }
                impl<T> Test<T> {
                    pub fn new(input: T) -> Self {
                        Self { _type: PhantomData }
                    }
                }
                
                fn _main() {
                    let _t: Test#{params:W} = Test::new($value);
                }
            """.trimIndent(), "params" to typeParameters)
        }
        testWriter.compileAndTest(clippy = true)
    }

    @Test
    fun `rustTypeParameters accepts RustType Unit`() {
       testItCompiles(rustTypeParameters(RustType.Unit), "()")
    }

    @Test
    fun `rustTypeParameters accepts Symbol`() {
        val runtimeType = RuntimeType("String", namespace = "std::string", dependency = null)
        testItCompiles(rustTypeParameters(runtimeType.toSymbol()), "\"test\".to_owned()")
    }

    @Test
    fun `rustTypeParameters accepts RuntimeType`() {
        val runtimeType = RuntimeType("String", namespace = "std::string", dependency = null)
        testItCompiles(rustTypeParameters(runtimeType), "\"test\".to_owned()")
    }

    @Test
    fun `rustTypeParameters accepts String`() {
        testItCompiles(rustTypeParameters("String"), "\"test\".to_owned()")
    }

    @Test
    fun `rustTypeParameters accepts GenericsGenerator`() {
        val gg = GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B"))
        val typeParameters = rustTypeParameters(gg)
        val testWriter = RustWriter.root()
        testWriter.withModule("test") {
            rustTemplate("""
                use std::marker::PhantomData;
                pub struct Test#{params:W} { _type: PhantomData<(A, B)> }
                impl#{params:W} Test#{params:W} {
                    pub fn new(input: (A, B)) -> Self {
                        Self { _type: PhantomData }
                    }
                }
                
                fn _main() {
                    let _t = Test::new((1.0f64, 1usize));
                }
            """.trimIndent(), "params" to typeParameters)
        }
        testWriter.compileAndTest(clippy = true)
    }
}
