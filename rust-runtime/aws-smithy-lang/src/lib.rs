/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Load, build, inspect, validate, traverse, and serialize Smithy 2.0 models.
//!
//! A [`Model`] is an immutable collection of owned semantic [shapes](shape::Shape), produced by
//! a [`ModelAssembler`] from any mix of [Smithy 2.0 IDL] and [JSON AST] documents, existing
//! models, and shapes built in code. Assembly injects the Smithy prelude, merges the inputs, and
//! [validates](validation) the result; every ordinary shape reference in a `Model` resolves.
//!
//! Shapes are looked up by [`ShapeId`]; members by name.
//!
//! ```
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let json = r#"{
//!     "smithy": "2.0",
//!     "shapes": {
//!         "ex#Service": {"type": "service", "resources": [{"target": "ex#Thing"}]},
//!         "ex#Thing": {"type": "resource", "read": {"target": "ex#GetThing"}},
//!         "ex#GetThing": {"type": "operation"}
//!     }
//! }"#;
//! let model = Model::from_json_str("model.json", json)?;
//! let service_id: ShapeId = "ex#Service".parse()?;
//! let service = model.expect_shape(&service_id)?.expect_service()?;
//! // Every operation in the service, including those bound through resources.
//! for operation in service.contained_operations() {
//!     println!("{} -> {}", operation.id(), operation.input());
//! }
//! assert_eq!(service.contained_operations().len(), 1);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! Models can also be built in code and combined with loaded documents:
//!
//! ```
//! use aws_smithy_lang::shape::{OperationShape, ServiceShape};
//! use aws_smithy_lang::{AstWriter, Model};
//!
//! let model = Model::assembler()
//!     .add_shape(ServiceShape::builder("ex#Service").add_operation("ex#Ping").build()?)
//!     .add_shape(OperationShape::builder("ex#Ping").build()?)
//!     .assemble()
//!     .into_result()?;
//! let json = AstWriter::new().to_string(&model)?;
//! assert!(json.starts_with(r#"{"smithy":"2.0","shapes":{"ex#Service""#));
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! Service and resource views distinguish *declaration-only* accessors such as
//! `direct_operations` from complete queries such as `contained_operations`. The
//! [`knowledge`] module provides the complete queries, as well as operation errors within a
//! service, service-scoped parents, and incoming relationships.
//!
//! IDL documents are loaded with [`Model::from_idl_str`] and friends or
//! [`ModelAssembler::add_idl_str`]. Relative shape IDs resolve against every input of an
//! assembly, so IDL and JSON documents can refer to each other:
//!
//! ```
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let idl = r#"
//! $version: "2"
//! namespace ex
//!
//! service Service { operations: [GetThing] }
//! "#;
//! let json = r#"{"smithy": "2.0", "shapes": {"ex#GetThing": {"type": "operation"}}}"#;
//! let model = Model::assembler()
//!     .add_idl_str("service.smithy", idl)
//!     .add_json_str("operations.json", json)
//!     .assemble()
//!     .into_result()?;
//! assert!(model.contains_shape(&ShapeId::new("ex#Service")?));
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! Inputs using `apply` or mixins are rejected with
//! [`DiagnosticCode::UnsupportedFeature`](diagnostic::DiagnosticCode).
//!
//! [Smithy 2.0 IDL]: https://smithy.io/2.0/spec/idl.html
//! [JSON AST]: https://smithy.io/2.0/spec/json-ast.html

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

pub mod diagnostic;
pub mod knowledge;
pub mod node;
pub mod prelude;
pub mod relationships;
pub mod shape;
pub mod traits;
pub mod traversal;
pub mod validation;

mod assembler;
mod ast_converter;
mod ast_writer;
mod idl_loader;
mod json_loader;
mod model;
mod shape_id;

pub use assembler::ModelAssembler;
pub use ast_writer::AstWriter;
pub use diagnostic::{LoadError, SourceLocation, WriteError};
pub use model::Model;
pub use node::{Node, NodeObject, NodeValue, Number};
pub use shape::{Shape, ShapeRef, ShapeType};
pub use shape_id::{IntoShapeId, InvalidShapeIdError, ShapeId};
pub use validation::{Validated, Validator};
