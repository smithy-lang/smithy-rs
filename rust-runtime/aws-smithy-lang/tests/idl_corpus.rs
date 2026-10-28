/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Every Smithy JSON AST model in `aws/sdk/aws-models`, rendered as equivalent Smithy IDL,
//! loads to the same model.

#[path = "support/to_idl.rs"]
mod to_idl;

use aws_smithy_lang::{AstWriter, Model};
use std::path::PathBuf;
use to_idl::{assemble, render, Layout};

fn corpus() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter(|path| {
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            value.get("smithy").is_some() && value.get("shapes").is_some()
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn corpus_models_match_as_idl() {
    let paths = corpus();
    assert!(!paths.is_empty());
    for path in &paths {
        let json = Model::from_json_file(path).unwrap();
        for layout in [Layout::PerNamespace, Layout::Files(4), Layout::Mixed] {
            let parts = render(&json, layout);
            let idl = assemble(&parts).unwrap_or_else(|e| {
                let shown: Vec<_> = e
                    .diagnostics()
                    .iter()
                    .take(5)
                    .map(|d| d.to_string())
                    .collect();
                panic!("{}: {e}\n{}", path.display(), shown.join("\n"))
            });
            assert!(
                idl.equivalent(&json),
                "{}: the IDL rendering is not equivalent",
                path.display()
            );
        }
        // Canonical output is identical, independent of representation.
        let idl = assemble(&render(&json, Layout::PerNamespace)).unwrap();
        let writer = AstWriter::new().canonical(true);
        assert_eq!(
            writer.to_string(&idl).unwrap(),
            writer.to_string(&json).unwrap(),
            "{}",
            path.display()
        );
    }
}
