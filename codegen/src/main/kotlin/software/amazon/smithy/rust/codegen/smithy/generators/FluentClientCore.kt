/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock

class FluentClientCore(private val model: Model) {
    fun RustWriter.renderVecHelper(member: MemberShape, memberName: String, coreType: RustType.Vec) {
        docs("Appends an item to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model)
        rustBlock("pub fn $memberName(mut self, inp: impl Into<${coreType.member.render(true)}>) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName(inp);
                self
                """
            )
        }
    }

    fun RustWriter.renderMapHelper(member: MemberShape, memberName: String, coreType: RustType.HashMap) {
        docs("Adds a key-value pair to `${member.memberName}`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model)
        val k = coreType.key
        val v = coreType.member

        rustBlock("pub fn $memberName(mut self, k: impl Into<${k.render()}>, v: impl Into<${v.render()}>) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName(k, v);
                self
                """
            )
        }
    }
}
