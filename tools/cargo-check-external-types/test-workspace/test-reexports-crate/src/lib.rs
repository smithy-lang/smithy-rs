/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub use external_lib::AssociatedGenericTrait;
pub use external_lib::ReprCType;
pub use external_lib::SimpleTrait;

pub mod Something {
    pub use external_lib::SimpleGenericTrait;
    pub use external_lib::SimpleNewType;
}

pub use external_lib::SomeOtherStruct;
pub use external_lib::SomeStruct;
