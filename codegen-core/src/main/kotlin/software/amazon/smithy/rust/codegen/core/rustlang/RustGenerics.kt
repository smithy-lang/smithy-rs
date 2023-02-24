/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

data class GenericTypeArg(
    val typeArg: String,
    val bound: RuntimeType? = null,
)

/**
 * A "writable" collection of Rust generic type args and their bounds.
 *
 * e.g.
 * ```
 * val generics = GenericsGenerator(
 *     GenericTypeArg("P", testRT("Pineapple")),
 *     GenericTypeArg("C", testRT("fruits::melon::Cantaloupe™")),
 *     GenericTypeArg("T"),
 * )
 *
 * rustTemplate("fn eat_fruit_salad#{decl}()", "decl" to generics.declaration())
 * // Writes "fn eat_fruit_salad<P, C, T>()"
 *
 * rustTemplate("fn eat_fruit_salad<#{decl}>()", "decl" to generics.declaration(withAngleBrackets = false))
 * // Writes "fn eat_fruit_salad<P, C, T>()"
 *
 * rustTemplate("""
 *     pub struct FruitSalad;
 *
 *     impl<#{decl}> FruitSalad
 *     where:
 *     #{bounds}
 *     {
 *         pub fn new#{decl}() { todo!() }
 *     }
 * """, "decl" to generics.declaration(), "bounds" to generics.bounds())
 * // Writes:
 * //     pub struct FruitSalad;
 * //
 * //     impl<P, C, T> FruitSalad
 * //     where:
 * //        P: Pineapple,
 * //        C: fruits::melon::Cantaloupe,
 * //     {
 * //         pub fn new<P, C, T>() { todo!() }
 * //     }
 * ```
 */
class RustGenerics(vararg genericTypeArgs: GenericTypeArg) {
    private val typeArgs: List<GenericTypeArg>

    init {
        typeArgs = genericTypeArgs.toList()
    }

    /**
     * Returns the generics type args formatted for use in declarations.
     *
     * e.g.
     *
     * ```
     * rustTemplate("fn eat_fruit_salad#{decl}()", "decl" to generics.declaration())
     * // Writes "fn eat_fruit_salad<P, C, T>()"
     *
     * rustTemplate("fn eat_fruit_salad<#{decl}>()", "decl" to generics.declaration(withAngleBrackets = false))
     * // Writes "fn eat_fruit_salad<P, C, T>()"
     * ```
     */
    fun declaration(withAngleBrackets: Boolean = true) =
        writable {
            // Write nothing if this generator is empty
            if (typeArgs.isNotEmpty()) {
                val typeArgs = typeArgs.joinToString(", ") { it.typeArg }

                if (withAngleBrackets) {
                    rustInlineTemplate("<")
                }

                rustInlineTemplate(typeArgs)

                if (withAngleBrackets) {
                    rustInlineTemplate(">")
                }
            }
        }

    /**
     * Returns bounded generic type args formatted for use in a "where" clause.
     * Type args with no bound will not be written.
     *
     * e.g.
     *
     * ```
     *  * rustTemplate("""
     *     pub struct FruitSalad;
     *
     *     impl<#{decl}> FruitSalad
     *     where:
     *     #{bounds}
     *     {
     *         pub fn new#{decl}() { todo!() }
     *     }
     * """, "decl" to generics.declaration(), "bounds" to generics.bounds())
     * // Writes:
     * //     pub struct FruitSalad;
     * //
     * //     impl<P, C, T> FruitSalad
     * //     where:
     * //        P: Pineapple,
     * //        C: fruits::melon::Cantaloupe,
     * //     {
     * //         pub fn new<P, C, T>() { todo!() }
     * //     }
     * ```
     */
    fun bounds() = writable {
        // Only write bounds for generic type params with a bound
        for ((typeArg, bound) in typeArgs) {
            if (bound != null) {
                rustTemplate("$typeArg: #{bound},\n", "bound" to bound)
            }
        }
    }

    /**
     * Combine two `GenericsGenerator`s into one. Type args for the first `GenericsGenerator` will appear before
     * type args from the second `GenericsGenerator`.
     *
     * e.g.
     *
     * ```
     * val ggA = GenericsGenerator(
     *     GenericTypeArg("A", testRT("Apple")),
     * )
     * val ggB = GenericsGenerator(
     *     GenericTypeArg("B", testRT("Banana")),
     * )
     *
     * rustTemplate("fn eat_fruit#{decl}()", "decl" to (ggA + ggB).declaration())
     * // Writes "fn eat_fruit<A, B>()"
     *
     */
    operator fun plus(operationGenerics: RustGenerics): RustGenerics {
        return RustGenerics(*(typeArgs + operationGenerics.typeArgs).toTypedArray())
    }
}
