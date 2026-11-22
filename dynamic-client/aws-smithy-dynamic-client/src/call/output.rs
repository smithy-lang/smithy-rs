/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::Document;
use std::fmt;

/// The output of one successful call.
pub struct DynamicOutput {
    members: Document,
}

impl DynamicOutput {
    pub(crate) fn new(members: Document) -> Self {
        Self { members }
    }

    /// Returns the output members, as a document object keyed by the Smithy member names of the
    /// output.
    pub fn members(&self) -> &Document {
        &self.members
    }

    /// Returns the output members, consuming the output.
    pub fn into_members(self) -> Document {
        self.members
    }
}

// The debug output has no output values, because a value can be `@sensitive`.
// TODO(dynamic-client): Print the output values and redact the `@sensitive` ones, as a generated
// output does, by walking the output schema.
impl fmt::Debug for DynamicOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicOutput").finish_non_exhaustive()
    }
}
