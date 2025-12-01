/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module hosts [`Layer`](tower::Layer)s that are generally meant to be applied _around_ the
//! [`Router`](crate::routing::Router), so they are enacted before a request is routed.

pub mod alb_health_check;
