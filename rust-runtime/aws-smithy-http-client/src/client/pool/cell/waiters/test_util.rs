/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Generated-sequence validation support for one cell's acquisition queue.
//!
//! The harness drives [`super::AcquisitionQueue`] directly and stands in for
//! the acquisition tasks, admission deliveries, returned senders, and
//! establishment tasks that would otherwise call it from behind the cell
//! lock. No cell exists and no lock is taken.

pub(super) mod operation_sequence;
pub(super) mod queue_harness;
