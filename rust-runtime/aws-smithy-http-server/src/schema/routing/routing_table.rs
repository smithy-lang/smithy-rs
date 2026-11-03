/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Associates protocol metadata with targets, including filtered and reordered target sets.

use super::{OperationTarget, RouterBuildError};
use std::collections::HashMap;

#[derive(Debug)]
struct RoutingEntry<M> {
    target: OperationTarget,
    metadata: M,
}

/// Owns metadata derived together with each target. Handler indices are never used as
/// local vector positions: a filtered view retains the original handler identities.
#[derive(Debug)]
pub(super) struct RoutingTable<M> {
    entries: Vec<RoutingEntry<M>>,
    positions: HashMap<usize, usize>,
}

impl<M> RoutingTable<M> {
    pub(super) fn try_new(
        targets: &[OperationTarget],
        mut metadata: impl FnMut(OperationTarget) -> Result<M, RouterBuildError>,
    ) -> Result<Self, RouterBuildError> {
        let mut entries = Vec::with_capacity(targets.len());
        let mut positions = HashMap::with_capacity(targets.len());
        for &target in targets {
            if positions.insert(target.index(), entries.len()).is_some() {
                return Err(RouterBuildError::Configuration(format!(
                    "duplicate routing target index {}",
                    target.index()
                )));
            }
            entries.push(RoutingEntry {
                target,
                metadata: metadata(target)?,
            });
        }
        Ok(Self { entries, positions })
    }

    pub(super) fn metadata(&self, target: OperationTarget) -> Option<&M> {
        let entry = &self.entries[*self.positions.get(&target.index())?];
        std::ptr::eq(entry.target.operation(), target.operation()).then_some(&entry.metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::OperationSchema;
    use aws_smithy_schema::{shape_id, Schema, ShapeType};

    static INPUT: Schema<'static> = Schema::new_struct(shape_id!("test", "Input"), ShapeType::Structure, &[]);
    static FIRST: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "First"), &INPUT, &INPUT, &[]);
    static SECOND: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "Second"), &INPUT, &INPUT, &[]);

    #[test]
    fn filtered_and_reordered_targets_keep_their_metadata() {
        // Metadata need not implement Clone, and storage does not grow with handler indices.
        struct Metadata(&'static str);
        let first = OperationTarget::new(1, &FIRST);
        let second = OperationTarget::new(usize::MAX, &SECOND);
        let table = RoutingTable::try_new(&[second, first], |target| {
            Ok(Metadata(target.operation().shape_id().shape_name()))
        })
        .unwrap();
        assert_eq!(table.metadata(first).unwrap().0, "First");
        assert_eq!(table.metadata(second).unwrap().0, "Second");
        assert_eq!(table.entries.len(), 2);
        assert!(table.metadata(OperationTarget::new(0, &FIRST)).is_none());
        assert!(table.metadata(OperationTarget::new(1, &SECOND)).is_none());
    }

    #[test]
    fn duplicate_indices_and_metadata_errors_reject_construction() {
        let first = OperationTarget::new(1, &FIRST);
        let second = OperationTarget::new(1, &SECOND);
        assert!(RoutingTable::try_new(&[first, second], |_| Ok(())).is_err());
        let result = RoutingTable::<()>::try_new(&[first], |_| {
            Err(RouterBuildError::Configuration("invalid metadata".into()))
        });
        assert!(matches!(result, Err(RouterBuildError::Configuration(message)) if message == "invalid metadata"));
        assert!(RoutingTable::try_new(&[], |_| Ok(()))
            .unwrap()
            .metadata(first)
            .is_none());
    }
}
