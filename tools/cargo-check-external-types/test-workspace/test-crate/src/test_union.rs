/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use external_lib::{ReprCType, SimpleTrait};

#[repr(C)]
pub union SimpleUnion {
    pub repr_c: ReprCType,
    pub something_else: u64,
}

impl SimpleUnion {
    pub fn repr_c(&self) -> &ReprCType {
        &self.repr_c
    }
}

#[repr(C)]
pub union GenericUnion<T: Copy + SimpleTrait> {
    pub repr_c: T,
    pub something_else: u64,
}
