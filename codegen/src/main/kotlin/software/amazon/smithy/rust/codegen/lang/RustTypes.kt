/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.lang

/**
 * A hierarchy of types handled by Smithy codegen
 */
sealed class RustType {

    /*
     * Name refers to the top-level type for import purposes
     */
    abstract val name: kotlin.String

    object Bool : RustType() {
        override val name: kotlin.String = "bool"
    }

    object String : RustType() {
        override val name: kotlin.String = "String"
    }

    data class Float(val precision: Int) : RustType() {
        override val name: kotlin.String = "f$precision"
    }

    data class Integer(val precision: Int) : RustType() {
        override val name: kotlin.String = "i$precision"
    }

    data class Vec(val member: RustType) : RustType() {
        override val name: kotlin.String = "Vec"
    }

    data class HashMap(val key: RustType, val value: RustType) : RustType() {
        // TODO: assert that underneath, the member is a String
        override val name: kotlin.String = "HashMap"
    }

    data class HashSet(val member: RustType) : RustType() {
        // TODO: assert that underneath, the member is a String
        override val name: kotlin.String = "HashSet"
    }

    data class Reference(val lifetime: kotlin.String?, val value: RustType) : RustType() {
        override val name: kotlin.String = value.name
    }

    data class Option(val value: RustType) : RustType() {
        override val name: kotlin.String = "Option"
    }

    data class Box(val value: RustType) : RustType() {
        override val name: kotlin.String = "Box"
    }

    data class Opaque(override val name: kotlin.String) : RustType()
}

fun RustType.render(): String = when (this) {
        is RustType.Bool -> this.name
        is RustType.Float -> this.name
        is RustType.Integer -> this.name
        is RustType.String -> this.name
        is RustType.Vec -> "${this.name}<${this.member.render()}>"
        is RustType.HashMap -> "${this.name}<${this.key.render()}, ${this.value.render()}>"
        is RustType.HashSet -> "${this.name}<${this.member.render()}>"
        is RustType.Reference -> "&${this.lifetime?.let { "'$it" } ?: ""} ${this.value.render()}"
        is RustType.Option -> "${this.name}<${this.value.render()}>"
        is RustType.Box -> "${this.name}<${this.value.render()}>"
        is RustType.Opaque -> this.name
}
