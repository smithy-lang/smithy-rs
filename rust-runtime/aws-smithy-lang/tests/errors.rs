/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors compose with Smithy runtime error handling.

use aws_smithy_lang::shape::ShapeExpectationError;
use aws_smithy_lang::traits::TraitDecodeError;
use aws_smithy_lang::{InvalidShapeIdError, LoadError, Model, ModelWriter, WriteError};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::error::display::DisplayErrorContext;
use std::error::Error;

/// Stand-in for a generated operation error.
#[derive(Debug)]
struct OperationError;

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation error")
    }
}

impl Error for OperationError {}

fn assert_error<T: Error + Send + Sync + 'static>() {}

#[test]
fn public_errors_are_send_sync_static() {
    assert_error::<LoadError>();
    assert_error::<WriteError>();
    assert_error::<TraitDecodeError>();
    assert_error::<ShapeExpectationError>();
    assert_error::<InvalidShapeIdError>();
}

#[test]
fn load_error_as_construction_failure() {
    let err = Model::from_json_str("model.json", "{\"smithy\": \"2.0\", oops}").unwrap_err();
    assert!(err.is_json_syntax());
    let sdk_error: SdkError<OperationError, ()> = SdkError::construction_failure(err);
    let rendered = DisplayErrorContext(&sdk_error).to_string();
    // The chain reaches the underlying serde_json error.
    assert!(
        rendered.contains("failed to construct request"),
        "{rendered}"
    );
    assert!(
        rendered.contains("failed to parse Smithy JSON AST"),
        "{rendered}"
    );
    assert!(rendered.contains("line 1"), "{rendered}");

    let source = sdk_error.source().unwrap();
    let load_error = source.downcast_ref::<LoadError>().unwrap();
    assert!(load_error.source().unwrap().is::<serde_json::Error>());
    assert_eq!(load_error.diagnostics().len(), 1);
}

#[test]
fn io_source_chain() {
    let err = Model::from_json_file("/definitely/missing/model.json").unwrap_err();
    assert!(err.source().unwrap().is::<std::io::Error>());
    let rendered = DisplayErrorContext(&err).to_string();
    assert!(
        rendered.contains("failed to read Smithy model"),
        "{rendered}"
    );

    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let model = Model::from_json_str("m", r#"{"smithy":"2.0"}"#).unwrap();
    let err = ModelWriter::new().write(&model, Failing).unwrap_err();
    let rendered = DisplayErrorContext(&err).to_string();
    assert!(rendered.contains("disk full"), "{rendered}");
}

#[test]
fn trait_decode_source_chain() {
    let err = TraitDecodeError::with_source("bad", std::io::Error::other("inner"));
    assert_eq!(err.source().unwrap().to_string(), "inner");
    let sdk_error: SdkError<OperationError, ()> = SdkError::construction_failure(err);
    assert!(DisplayErrorContext(&sdk_error)
        .to_string()
        .contains("inner"));
}
