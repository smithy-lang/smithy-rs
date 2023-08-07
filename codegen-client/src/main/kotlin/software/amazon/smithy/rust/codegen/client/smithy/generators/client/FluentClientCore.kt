/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asArgument
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

class FluentClientCore(private val model: Model) {
    /** Generate and write Rust code for a builder method that sets a Vec<T> */
    fun RustWriter.renderVecHelper(member: MemberShape, memberName: String, coreType: RustType.Vec) {
        docs("Appends an item to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        val input = coreType.member.asArgument("input")

        documentShape(member, model)
        deprecatedShape(member)
        rustBlock("pub fn $memberName(mut self, ${input.argument}) -> Self") {
            write("self.inner = self.inner.$memberName(${input.value});")
            write("self")
        }
    }

    /** Generate and write Rust code for a builder method that sets a HashMap<K,V> */
    fun RustWriter.renderMapHelper(member: MemberShape, memberName: String, coreType: RustType.HashMap) {
        docs("Adds a key-value pair to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        val k = coreType.key.asArgument("k")
        val v = coreType.member.asArgument("v")

        documentShape(member, model)
        deprecatedShape(member)
        rustBlock("pub fn $memberName(mut self, ${k.argument}, ${v.argument}) -> Self") {
            write("self.inner = self.inner.$memberName(${k.value}, ${v.value});")
            write("self")
        }
    }

    /**
     * Generate and write Rust code for a builder method that sets an input. Can be used for setter methods as well e.g.
     *
     * `renderInputHelper(memberShape, "foo", RustType.String)` -> `pub fn foo(mut self, input: impl Into<String>) -> Self { ... }`
     * `renderInputHelper(memberShape, "set_bar", RustType.Option)` -> `pub fn set_bar(mut self, input: Option<String>) -> Self { ... }`
     */
    fun RustWriter.renderInputHelper(member: MemberShape, memberName: String, coreType: RustType) {
        val functionInput = coreType.asArgument("input")

        documentShape(member, model)
        deprecatedShape(member)
        rustBlock("pub fn $memberName(mut self, ${functionInput.argument}) -> Self") {
            write("self.inner = self.inner.$memberName(${functionInput.value});")
            write("self")
        }
    }

    /**
     * Generate and write Rust code for a getter method that returns a reference to the inner data.
     */
    fun RustWriter.renderGetterHelper(member: MemberShape, memberName: String, coreType: RustType) {
        documentShape(member, model)
        deprecatedShape(member)
        withBlockTemplate("pub fn $memberName(&self) -> &#{CoreType} {", "}", "CoreType" to coreType) {
            write("self.inner.$memberName()")
        }
    }
}
