/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Generated-sequence validation support for one bounded origin's admission.
//!
//! The harness drives [`super::AdmissionState`] directly. Prepared crossings
//! that production hands to detached actions are held in harness slots and
//! settled by later operations. No cell exists and no lock is taken.

pub(super) mod model_harness;
pub(super) mod operation_sequence;
