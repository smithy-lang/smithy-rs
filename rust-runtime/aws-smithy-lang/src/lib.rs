/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Load, inspect, traverse, and serialize Smithy 2.0 JSON AST models.
//!
//! Version `0.1` loads a single [Smithy 2.0 JSON AST] document, injects the Smithy prelude,
//! and validates the result under the `StructuralV1` profile. Every loaded [`Model`] is
//! immutable and every ordinary shape reference in it resolves. Documents using `apply` or
//! mixins are rejected with [`DiagnosticCode::UnsupportedFeature`](diagnostic::DiagnosticCode).
//!
//! ```
//! use aws_smithy_lang::Model;
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
//! let service = model.expect_shape("ex#Service")?.expect_service()?;
//! // Every operation in the service, including those bound through resources.
//! for operation in service.contained_operations() {
//!     println!("{} -> {}", operation.id(), operation.input());
//! }
//! assert_eq!(service.contained_operations().len(), 1);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! Service and resource views distinguish *declaration-only* accessors such as
//! `direct_operations` from complete queries such as `contained_operations`. The
//! [`knowledge`] module provides the complete queries, as well as operation errors within a
//! service, service-scoped parents, and incoming relationships.
//!
//! [Smithy 2.0 JSON AST]: https://smithy.io/2.0/spec/json-ast.html

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
pub mod shape;
pub mod traits;
pub mod traversal;

mod ast;
mod loader;
mod model;
mod prelude;
mod shape_id;
mod validate;
mod writer;

pub use diagnostic::{LoadError, WriteError};
pub use loader::ModelLoader;
pub use model::Model;
pub use node::{Node, NodeObject, Number};
pub use shape::{ShapeRef, ShapeType};
pub use shape_id::{InvalidShapeIdError, ShapeId};
pub use writer::ModelWriter;
