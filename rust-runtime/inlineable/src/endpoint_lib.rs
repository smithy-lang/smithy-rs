/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */
// Not all crates that inline these modules will use all of these functions
// TODO(Zelda) Figure out how to avoid inlining code that doesn't get used.
#![allow(unused)]

pub(crate) mod arn;
pub(crate) mod diagnostic;
pub(crate) mod host;
pub(crate) mod parse_url;
pub(crate) mod partition;
pub(crate) mod substring;
pub(crate) mod uri_encode;
// TODO(Zelda) SDK stuff shouldn't be here
pub(crate) mod s3;
