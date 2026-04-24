/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool implementation for the v2 HTTP client.
//!
//! This module composes hyper-util's pool primitives (Cache, Singleton, Negotiate, Map)
//! with SDK-owned connection lifecycle management including:
//! - Connection state tracking (idle-since, remote IP, poisoned flag)
//! - Proactive health checks on checkout
//! - Connection poisoning support
//! - Queryable pool state (idle/active counts)
