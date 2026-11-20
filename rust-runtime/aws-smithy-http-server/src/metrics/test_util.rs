/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{operation::OperationShape, service::ServiceShape, shape_id::ShapeId};

/// A service shape for tests.
pub(crate) struct TestService;

impl ServiceShape for TestService {
    const ID: ShapeId = ShapeId::new("test#TestService", "test", "TestService");
    const VERSION: Option<&'static str> = Some("2026-11-12");
    type Protocol = ();
    type Operations = ();
}

/// A service shape for tests, with the empty version of a model that does not specify one.
pub(crate) struct UnversionedTestService;

impl ServiceShape for UnversionedTestService {
    const ID: ShapeId = ShapeId::new("test#UnversionedTestService", "test", "UnversionedTestService");
    const VERSION: Option<&'static str> = Some("");
    type Protocol = ();
    type Operations = ();
}

/// An operation shape for tests.
pub(crate) struct TestOperation;

impl OperationShape for TestOperation {
    const ID: ShapeId = ShapeId::new("test#TestOperation", "test", "TestOperation");
    type Input = ();
    type Output = ();
    type Error = ();
}
