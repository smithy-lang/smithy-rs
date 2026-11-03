/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_lang::{AstWriter, LoadError, Model, ModelAssembler};

/// Assembles with both default and deliberately low limits, so limit checks are exercised as
/// well as the grammar.
pub fn assemble(build: impl Fn(ModelAssembler) -> ModelAssembler) {
    let low = Model::assembler()
        .max_depth(4)
        .max_shapes(8)
        .max_diagnostics(4)
        .max_input_bytes(4096);
    check(build(Model::assembler()).assemble().into_result());
    check(build(low).assemble().into_result());
}

/// Every accepted model must survive a write and read through the JSON AST, and every error
/// must render.
fn check(result: Result<Model, LoadError>) {
    match result {
        Ok(model) => {
            let json = AstWriter::new().to_string(&model).unwrap();
            let reloaded = Model::from_json_str("written.json", &json)
                .unwrap_or_else(|e| panic!("written model does not load: {e}: {json}"));
            assert!(model.equivalent(&reloaded), "{json}");
        }
        Err(err) => {
            let _ = err.to_string();
            for diagnostic in err.diagnostics() {
                let _ = diagnostic.to_string();
            }
        }
    }
}
