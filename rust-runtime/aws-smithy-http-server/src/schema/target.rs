/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::{Schema, ShapeId, Trait};
use std::any::Any;

const TARGET_ID: ShapeId<'static> =
    ShapeId::from_parts("smithy.rust.server#targetSchema", "smithy.rust.server", "targetSchema");

/// Server metadata linking an aggregate member to its target shape schema.
///
/// Member schemas retain their names and binding traits. This metadata supplies the
/// target identity and shape traits when framing events or nested modeled errors.
#[derive(Clone, Copy)]
pub struct TargetSchema(&'static Schema<'static>);

impl TargetSchema {
    /// Associates a member with its generated target schema.
    pub const fn new(schema: &'static Schema<'static>) -> Self {
        Self(schema)
    }

    /// Resolves a member's target, or returns the supplied shape schema unchanged.
    pub fn resolve<'s, 'a>(schema: &'s Schema<'a>) -> &'s Schema<'a> {
        schema
            .traits()
            .and_then(|traits| traits.get_fqn(TARGET_ID.as_str()))
            .and_then(|value| value.as_any().downcast_ref::<Self>())
            .map_or(schema, |target| target.0)
    }
}

impl std::fmt::Debug for TargetSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Avoid traversing recursive schema graphs.
        f.debug_tuple("TargetSchema").field(self.0.shape_id()).finish()
    }
}

impl Trait for TargetSchema {
    fn trait_id(&self) -> &ShapeId<'static> {
        &TARGET_ID
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType, TraitMap};
    use std::sync::LazyLock;

    static TARGET_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut traits = TraitMap::new();
        traits.insert(Box::new(TargetSchema::new(&NODE)));
        traits
    });
    static CHILD: Schema<'static> =
        Schema::new_member(shape_id!("test", "Node", "child"), ShapeType::Structure, "child", 0)
            .with_traits(&TARGET_TRAITS)
            .with_event_payload();
    static NODE: Schema<'static> = Schema::new_struct(shape_id!("test", "Node"), ShapeType::Structure, &[&CHILD]);

    #[test]
    fn recursive_targets_preserve_member_metadata() {
        assert_eq!(CHILD.member_name(), Some("child"));
        assert!(CHILD.event_payload());
        assert!(std::ptr::eq(TargetSchema::resolve(&CHILD), &NODE));
        assert!(std::ptr::eq(TargetSchema::resolve(&NODE), &NODE));
        assert!(std::ptr::eq(TargetSchema::resolve(NODE.members()[0]), &NODE));
        assert!(format!("{:?}", TargetSchema::new(&NODE)).contains("test#Node"));
    }

    #[test]
    fn shape_schemas_can_borrow_runtime_data() {
        let name = String::from("RuntimeShape");
        let id = format!("test#{name}");
        let schema = Schema::new(ShapeId::from_parts(&id, "test", &name), ShapeType::Structure);
        assert!(std::ptr::eq(TargetSchema::resolve(&schema), &schema));
    }
}
