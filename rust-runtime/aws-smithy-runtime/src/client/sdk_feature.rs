/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::config_bag::{Storable, StoreAppend};

#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum SmithySdkFeature {
    ProtocolRpcV2Cbor,
}

impl Storable for SmithySdkFeature {
    type Storer = StoreAppend<Self>;
}
