/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::Document;
use std::fmt;

/// The input of one call.
// TODO(dynamic-client): Add the `@streaming` payload of the input.
pub struct DynamicInput {
    members: Document,
}

impl DynamicInput {
    /// Creates an input from the input members, as a document object keyed by the Smithy member
    /// names of the input.
    pub fn new(members: Document) -> Self {
        Self { members }
    }

    /// Returns the input members.
    pub fn members(&self) -> &Document {
        &self.members
    }

    /// Returns the input members, consuming the input.
    pub fn into_members(self) -> Document {
        self.members
    }
}

impl From<Document> for DynamicInput {
    fn from(members: Document) -> Self {
        Self::new(members)
    }
}

// The debug output has no input values, because a value can be `@sensitive`.
impl fmt::Debug for DynamicInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicInput").finish_non_exhaustive()
    }
}
