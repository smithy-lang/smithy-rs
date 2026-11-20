/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Mixin resolution: computes the members and traits that shapes inherit from their mixins,
//! following the reference implementation's loader (`LoaderShapeMap`, `ApplyMixin`, and the
//! shape builders).
//!
//! Until this runs, a shape with mixins holds its declared form: its own traits and the members
//! it declares. Traits applied to members it does not declare are held separately, because the
//! member may be inherited.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSet, SourceLocation};
use crate::node::Node;
use crate::shape::{InheritedMember, MemberShape, Members, Shape, TraitMap};
use crate::shape_id::ShapeId;
use crate::traits;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

/// The inputs of [`resolve`].
pub(super) struct Resolution<'a> {
    /// Every shape, with shapes that have mixins in their declared form.
    pub(super) shapes: &'a mut IndexMap<ShapeId, Shape>,
    /// The prelude, whose shapes may also be used as mixins.
    pub(super) prelude: Option<&'a IndexMap<ShapeId, Shape>>,
    /// Traits applied to members that shapes with mixins do not declare, by member ID.
    pub(super) inherited_traits: IndexMap<ShapeId, TraitMap>,
    /// `$name` members whose targets come from mixins, by root shape ID.
    pub(super) elided: HashMap<ShapeId, Vec<ElidedMembers>>,
    /// Later definitions of shapes with mixins, by shape ID. Like the reference implementation,
    /// which compares built shapes, they are compared with the kept definition once both are
    /// resolved.
    pub(super) duplicates: HashMap<ShapeId, Vec<Duplicate>>,
    pub(super) diagnostics: &'a mut DiagnosticSet,
}

/// The `$name` members of one definition of a shape with mixins whose targets come from a mixin.
/// Like the reference implementation, each definition checks its own.
#[derive(Default)]
pub(super) struct ElidedMembers {
    /// The location of the definition, which identifies it: an IDL document defines a shape once.
    pub(super) definition: SourceLocation,
    /// Where each member is declared, by name.
    pub(super) members: IndexMap<String, SourceLocation>,
}

/// A later definition of a shape with mixins.
///
/// Every trait applied to a member, including the traits written on any definition's member, is
/// merged into one place: the kept definition's member if it declares it, or else the held traits.
/// As in the reference implementation, which applies each trait to every definition, a later
/// definition's member has the traits from both places.
pub(super) struct Duplicate {
    /// The definition in its declared form, with the applied traits of the shape and of the
    /// members the kept definition also declares.
    pub(super) shape: Shape,
    /// The applied traits of the members the kept definition declares and this one does not, by
    /// member ID.
    pub(super) kept_member_traits: IndexMap<ShapeId, TraitMap>,
}

/// The results of [`resolve`] that the assembler still has to act on.
pub(super) struct Outcome {
    /// The held traits that no inherited member claimed, by member ID.
    pub(super) unclaimed: IndexMap<ShapeId, TraitMap>,
    /// The resolved later definitions of shapes that resolved, to compare with them.
    pub(super) duplicates: Vec<Shape>,
}

/// Resolves the mixins of every shape that has them, mixins before the shapes that use them.
///
/// As in the reference implementation:
///
/// - Every shape in a mixin cycle is reported and dropped. Shapes that depend on one are dropped
///   without a diagnostic. (The reference implementation drops a shape that is its own mixin
///   without a diagnostic too; it is reported here like any other cycle.)
/// - A shape whose mixin is missing (or was dropped), whose mixins are of another shape type, or
///   whose mixins provide a member with different targets is reported and dropped.
/// - Members inherited from mixins come first, in mixin order and depth first, then the
///   members the shape declares. A declared member with the name of an inherited one redefines
///   it in place; a different target is reported, and the declared target is kept.
/// - Traits override, and never merge: a later mixin's over an earlier one's, and the shape's
///   (or member's) own over inherited ones. A mixin's `@mixin` trait and its `localTraits` are
///   not inherited.
pub(super) fn resolve(resolution: Resolution<'_>) -> Outcome {
    let Resolution {
        shapes,
        prelude,
        mut inherited_traits,
        mut elided,
        mut duplicates,
        diagnostics,
    } = resolution;
    let pending: Vec<ShapeId> = shapes
        .values()
        .filter(|shape| !shape.mixins().is_empty())
        .map(|shape| shape.id().clone())
        .collect();
    let mut resolved_duplicates = Vec::new();
    if pending.is_empty() {
        return Outcome {
            unclaimed: inherited_traits,
            duplicates: resolved_duplicates,
        };
    }
    let in_cycle = report_cycles(shapes, &pending, diagnostics);
    let no_elided_members = IndexMap::new();
    let order = dependency_order(shapes, &pending, &in_cycle);

    // Shapes that were not built, and among them those dropped without a diagnostic.
    let mut dropped: HashSet<ShapeId> = in_cycle.clone();
    let mut silent = in_cycle;
    for id in order {
        if shapes[&id].mixins().iter().any(|m| silent.contains(m)) {
            silent.insert(id.clone());
            dropped.insert(id);
            continue;
        }
        let elided_definitions = elided.remove(&id).unwrap_or_default();
        let elided_members = |shape: &Shape| {
            elided_definitions
                .iter()
                .find(|elided| elided.definition == *shape.source_location())
                .map_or(&no_elided_members, |elided| &elided.members)
        };
        // Later definitions first: they read the held traits, which the kept one then claims.
        let mut others = Vec::new();
        for Duplicate {
            shape: mut duplicate,
            kept_member_traits,
        } in duplicates.remove(&id).unwrap_or_default()
        {
            let declared = Declared::take_from(&mut duplicate);
            let resolved = Inheritance {
                shape: &duplicate,
                declared,
                held: HeldTraits::Read {
                    held: &inherited_traits,
                    kept_member_traits: &kept_member_traits,
                },
                elided: elided_members(&duplicate),
                diagnostics: &mut *diagnostics,
            }
            .resolve(shapes, prelude, &dropped);
            if let Some(resolved) = resolved {
                resolved.put_into(&mut duplicate);
                others.push(duplicate);
            }
        }
        let declared = Declared::take_from(&mut shapes[&id]);
        let resolved = Inheritance {
            shape: &shapes[&id],
            declared,
            held: HeldTraits::Claim(&mut inherited_traits),
            elided: elided_members(&shapes[&id]),
            diagnostics: &mut *diagnostics,
        }
        .resolve(shapes, prelude, &dropped);
        match resolved {
            Some(resolved) => {
                resolved.put_into(&mut shapes[&id]);
                resolved_duplicates.extend(others);
            }
            None => {
                dropped.insert(id.clone());
            }
        }
        // The traits written on `$name` members that no definition claimed go with them, like
        // the other traits of a definition: the reference implementation applies them to the
        // definition that declares the member.
        discard_elided(&mut inherited_traits, &id, &elided_definitions);
    }
    // The shapes that were not resolved, in or after a cycle.
    for (id, definitions) in elided {
        discard_elided(&mut inherited_traits, &id, &definitions);
    }
    shapes.retain(|id, _| !dropped.contains(id));
    // One pass for the entries `take_held` emptied.
    inherited_traits.retain(|_, traits| !traits.is_empty());
    Outcome {
        unclaimed: inherited_traits,
        duplicates: resolved_duplicates,
    }
}

/// Takes the traits held for `member`, leaving an empty entry that [`resolve`] drops at the end.
///
/// `shift_remove` would keep the map compact, but it is O(n) because it shifts every later entry,
/// which makes resolution quadratic in the number of held traits (2.6 s instead of 0.27 s for 40,000
/// `apply` statements to inherited members). `swap_remove` is O(1) but reorders the map, and its
/// order is the order in which unclaimed traits are reported.
fn take_held(inherited_traits: &mut IndexMap<ShapeId, TraitMap>, member: &ShapeId) -> TraitMap {
    inherited_traits
        .get_mut(member)
        .map(std::mem::take)
        .unwrap_or_default()
}

/// The applied traits of the members a definition does not declare, as that definition uses them.
enum HeldTraits<'a> {
    /// The kept definition claims the held traits of the members it inherits.
    Claim(&'a mut IndexMap<ShapeId, TraitMap>),
    /// A later definition, resolved only to be compared, reads them (see [`Duplicate`]).
    Read {
        held: &'a IndexMap<ShapeId, TraitMap>,
        kept_member_traits: &'a IndexMap<ShapeId, TraitMap>,
    },
}

impl HeldTraits<'_> {
    /// The applied traits of `member` that its definition does not hold; claimed ones are taken.
    fn own_traits(&mut self, member: &ShapeId) -> TraitMap {
        match self {
            HeldTraits::Claim(held) => take_held(held, member),
            HeldTraits::Read {
                held,
                kept_member_traits,
            } => {
                let mut traits = held.get(member).cloned().unwrap_or_default();
                if let Some(kept) = kept_member_traits.get(member) {
                    traits.extend(kept.iter().map(|(id, value)| (id.clone(), value.clone())));
                }
                traits
            }
        }
    }

    /// Drops the traits held for `member` without a diagnostic, when they go with the member.
    fn discard(&mut self, member: &ShapeId) {
        if let HeldTraits::Claim(held) = self {
            take_held(held, member);
        }
    }

    /// Returns `true` if `member` has applied traits that its definition does not hold.
    fn contains(&self, member: &ShapeId) -> bool {
        let has = |map: &IndexMap<ShapeId, TraitMap>| {
            map.get(member).is_some_and(|traits| !traits.is_empty())
        };
        match self {
            HeldTraits::Claim(held) => has(held),
            HeldTraits::Read {
                held,
                kept_member_traits,
            } => has(held) || has(kept_member_traits),
        }
    }
}

/// Drops the held traits of the `$name` members of `root`'s definitions.
fn discard_elided(
    inherited_traits: &mut IndexMap<ShapeId, TraitMap>,
    root: &ShapeId,
    definitions: &[ElidedMembers],
) {
    for name in definitions.iter().flat_map(|elided| elided.members.keys()) {
        take_held(inherited_traits, &member_id(root, name));
    }
}

fn member_id(root: &ShapeId, name: &str) -> ShapeId {
    root.with_member(name)
        .expect("member names of loaded shapes are valid")
}

/// Reports every shape in a mixin cycle, and returns them.
///
/// Like the reference implementation's `DependencyGraph.findCycles`, this runs Tarjan's
/// algorithm over the edges from each mixin to the shapes that use it, starting from shapes in
/// the order they were first seen, and lists each cycle in the order it is found. The reference
/// implementation sees shapes in hash order, so its lists can be in another order.
fn report_cycles(
    shapes: &IndexMap<ShapeId, Shape>,
    pending: &[ShapeId],
    diagnostics: &mut DiagnosticSet,
) -> HashSet<ShapeId> {
    // Nodes in the order they are first seen, and the shapes that use each one as a mixin.
    let mut nodes: IndexMap<&ShapeId, Vec<usize>> = IndexMap::new();
    for id in pending {
        let entry = nodes.entry(id);
        let user = entry.index();
        entry.or_default();
        for mixin in shapes[id].mixins() {
            // A shape lists each mixin once (the loaders drop repeats), so a repeated edge can
            // only be the one just added. Checking every user instead would be quadratic in the
            // number of shapes that use one mixin.
            let users = nodes.entry(mixin).or_default();
            if users.last() != Some(&user) {
                users.push(user);
            }
        }
    }

    // Iterative Tarjan: `index[v]` and `low[v]` are `None` until `v` is visited.
    let count = nodes.len();
    let mut index: Vec<Option<usize>> = vec![None; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..count {
        if index[start].is_some() {
            continue;
        }
        // `(node, next user to visit)`
        let mut path = vec![(start, 0)];
        index[start] = Some(next);
        low[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;
        while let Some(&mut (node, ref mut child)) = path.last_mut() {
            if let Some(&user) = nodes[node].get(*child) {
                *child += 1;
                match index[user] {
                    None => {
                        index[user] = Some(next);
                        low[user] = next;
                        next += 1;
                        stack.push(user);
                        on_stack[user] = true;
                        path.push((user, 0));
                    }
                    Some(user_index) if on_stack[user] => {
                        low[node] = low[node].min(user_index);
                    }
                    Some(_) => {}
                }
                continue;
            }
            path.pop();
            if let Some(&(parent, _)) = path.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if Some(low[node]) == index[node] {
                let mut cycle = Vec::new();
                loop {
                    let member = stack.pop().expect("the node is on the stack");
                    on_stack[member] = false;
                    cycle.push(member);
                    if member == node {
                        break;
                    }
                }
                // The reference implementation ignores a shape that is its own mixin here (and
                // then drops it without a diagnostic); it is reported like any other cycle.
                if cycle.len() > 1 || nodes[node].contains(&node) {
                    cycles.push(cycle);
                }
            }
        }
    }

    let mut in_cycle = HashSet::new();
    for cycle in cycles {
        let ids: Vec<&ShapeId> = cycle
            .iter()
            .map(|&i| *nodes.get_index(i).unwrap().0)
            .collect();
        for &id in &ids {
            let others: Vec<String> = ids
                .iter()
                .filter(|other| **other != id)
                .map(|other| format!("`{other}`"))
                .collect();
            let message = if others.is_empty() {
                format!("cannot resolve the mixins of `{id}`: it uses itself as a mixin")
            } else {
                format!(
                    "cannot resolve the mixins of `{id}`: it is in a mixin cycle with {}",
                    others.join(", ")
                )
            };
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::InvalidShape, message)
                    .with_shape_id(id.clone())
                    .with_source_location(shapes[id].source_location().child("/mixins")),
            );
            in_cycle.insert(id.clone());
        }
    }
    in_cycle
}

/// The shapes with mixins, each after the shapes with mixins that it uses, other than shapes in
/// a cycle.
fn dependency_order(
    shapes: &IndexMap<ShapeId, Shape>,
    pending: &[ShapeId],
    in_cycle: &HashSet<ShapeId>,
) -> Vec<ShapeId> {
    let is_pending = |id: &ShapeId| {
        !in_cycle.contains(id) && shapes.get(id).is_some_and(|s| !s.mixins().is_empty())
    };
    let mut order = Vec::with_capacity(pending.len());
    let mut visited: HashSet<&ShapeId> = HashSet::with_capacity(pending.len());
    for start in pending.iter().filter(|id| is_pending(id)) {
        if !visited.insert(start) {
            continue;
        }
        // `(shape, next mixin to visit)`
        let mut path = vec![(start, 0)];
        while let Some(&mut (id, ref mut next)) = path.last_mut() {
            match shapes[id].mixins().get(*next) {
                Some(mixin) => {
                    *next += 1;
                    if is_pending(mixin) && visited.insert(mixin) {
                        path.push((mixin, 0));
                    }
                }
                None => {
                    order.push(id.clone());
                    path.pop();
                }
            }
        }
    }
    order
}

/// The parts of a shape with mixins that it declares, taken out of the shape while it is resolved,
/// so that resolving it moves them instead of copying the shape.
struct Declared {
    /// The shape's own traits.
    traits: TraitMap,
    /// The members it declares.
    members: Members,
}

impl Declared {
    fn take_from(shape: &mut Shape) -> Self {
        Self {
            traits: std::mem::take(&mut shape.common_mut().traits),
            members: std::mem::take(
                shape
                    .named_members_mut()
                    .expect("only shapes with named members have mixins"),
            ),
        }
    }
}

/// The resolved parts of a shape with mixins.
struct Resolved {
    /// Inherited traits, overridden by the shape's own.
    traits: TraitMap,
    /// The IDs of the shape's own traits.
    introduced: HashSet<ShapeId>,
    /// Inherited members, then the shape's own.
    members: Members,
}

impl Resolved {
    /// Puts the resolved parts into the shape they were taken from.
    fn put_into(self, shape: &mut Shape) {
        let common = shape.common_mut();
        common.traits = self.traits;
        common
            .mixins
            .as_mut()
            .expect("shapes being resolved have mixins")
            .introduced = self.introduced;
        *shape
            .named_members_mut()
            .expect("only shapes with named members have mixins") = self.members;
    }
}

/// One definition of a shape being resolved, with the parts of the resolution it reads or
/// updates.
struct Inheritance<'a> {
    /// The shape, without the parts in [`declared`](Self::declared): its ID, type, location, and
    /// mixins.
    shape: &'a Shape,
    declared: Declared,
    /// Traits held for members the shape does not declare; see [`Resolution`].
    held: HeldTraits<'a>,
    /// The shape's `$name` members whose targets come from a mixin.
    elided: &'a IndexMap<String, SourceLocation>,
    diagnostics: &'a mut DiagnosticSet,
}

/// A shape's members in their resolved order, before their traits are resolved.
struct MemberLayout<'a> {
    /// Inherited members first, in mixin order, then the shape's own. Inherited members still
    /// have only their own traits.
    members: Members,
    /// Whether each member, by index, is one the shape declares.
    is_local: Vec<bool>,
    /// The traits each inherited member inherits, by member name: its mixin members' traits, a
    /// later mixin's overriding an earlier one's.
    inherited: HashMap<&'a str, TraitMap>,
}

impl<'a> Inheritance<'a> {
    /// The resolved parts of the shape, or `None` (after reporting why) if it cannot be built.
    fn resolve(
        mut self,
        shapes: &'a IndexMap<ShapeId, Shape>,
        prelude: Option<&'a IndexMap<ShapeId, Shape>>,
        dropped: &HashSet<ShapeId>,
    ) -> Option<Resolved> {
        let mixins = self.find_mixins(shapes, prelude, dropped)?;
        self.check_mixin_types(&mixins)?;
        let layout = self.lay_out_members(&mixins)?;
        let members = self.resolve_member_traits(layout);
        self.check_elided_members(&members);
        let own = std::mem::take(&mut self.declared.traits);
        Some(Resolved {
            introduced: own.keys().cloned().collect(),
            traits: shape_traits(&mixins, own),
            members,
        })
    }

    fn id(&self) -> &'a ShapeId {
        self.shape.id()
    }

    fn error(&self, code: DiagnosticCode, message: String, location: SourceLocation) -> Diagnostic {
        Diagnostic::error(code, message)
            .with_shape_id(self.id().clone())
            .with_source_location(location)
    }

    /// The shape's mixins, which are resolved already (or in the prelude). A missing mixin, or one
    /// that could not be built, is reported.
    fn find_mixins(
        &mut self,
        shapes: &'a IndexMap<ShapeId, Shape>,
        prelude: Option<&'a IndexMap<ShapeId, Shape>>,
        dropped: &HashSet<ShapeId>,
    ) -> Option<Vec<&'a Shape>> {
        let (id, shape) = (self.id(), self.shape);
        let mut mixins = Vec::with_capacity(shape.mixins().len());
        for (index, mixin) in shape.mixins().iter().enumerate() {
            let found = (!dropped.contains(mixin))
                .then(|| shapes.get(mixin).or_else(|| prelude?.get(mixin)))
                .flatten();
            let Some(found) = found else {
                let error = self.error(
                    DiagnosticCode::UnresolvedReference,
                    format!("cannot apply mixin `{mixin}` to `{id}`: `{mixin}` not found"),
                    shape
                        .source_location()
                        .child(&format!("/mixins/{index}/target")),
                );
                self.diagnostics.push(error);
                return None;
            };
            mixins.push(found);
        }
        Some(mixins)
    }

    /// Reports mixins of another shape type than the shape's, all at once.
    fn check_mixin_types(&mut self, mixins: &[&Shape]) -> Option<()> {
        let shape_type = self.shape.shape_type();
        let mut wrong: Vec<&str> = mixins
            .iter()
            .filter(|mixin| mixin.shape_type() != shape_type)
            .map(|mixin| mixin.id().as_str())
            .collect();
        if wrong.is_empty() {
            return Some(());
        }
        wrong.sort_unstable();
        let error = self.error(
            DiagnosticCode::InvalidTarget,
            format!(
                "the mixins of {shape_type} `{}` must be {shape_type} shapes, but these are not: \
                 `{}`",
                self.id(),
                wrong.join("`, `")
            ),
            self.shape.source_location().child("/mixins"),
        );
        self.diagnostics.push(error);
        None
    }

    /// Orders the members: inherited ones first, in mixin order (each mixin's members are already
    /// in this order), then the shape's own.
    ///
    /// A member the shape declares with the name of an inherited one redefines it in place; a
    /// different target is reported, and the declared one is kept. Mixins that provide a member
    /// with different targets are reported, and the shape cannot be built, unless the member has
    /// its own traits (applied, or written on its `$name` declaration): like the reference
    /// implementation, which then makes a copy of the first mixin's member, the conflict is
    /// reported on the member and it keeps the first mixin's target.
    fn lay_out_members(&mut self, mixins: &[&'a Shape]) -> Option<MemberLayout<'a>> {
        let mut local = std::mem::take(&mut self.declared.members);
        let mut members = Members::with_capacity(local.len());
        let mut is_local = Vec::with_capacity(local.len());
        let mut inherited: HashMap<&'a str, TraitMap> = HashMap::new();
        for mixin_member in mixins.iter().flat_map(|mixin| mixin.members()) {
            let name = mixin_member.name();
            let traits = inherited.entry(name).or_default();
            for (trait_id, value) in &mixin_member.traits {
                traits.insert(trait_id.clone(), value.clone());
            }
            if let Some(own) = local.get_mut(name) {
                if own.target != mixin_member.target {
                    let location = own.source.child("/target");
                    self.diagnostics
                        .push(member_conflict(own, location, mixin_member));
                }
                if !members.contains_key(name) {
                    // Moved, not copied; the stub left behind is skipped below.
                    let stub =
                        MemberShape::new(own.id.clone(), own.target.clone(), own.source.clone());
                    members.insert(name.to_owned(), std::mem::replace(own, stub));
                    is_local.push(true);
                }
            } else if let Some(previous) = members.get_mut(name) {
                let has_own_traits =
                    self.held.contains(&previous.id) || self.elided.contains_key(name);
                if previous.target != mixin_member.target {
                    if has_own_traits {
                        // A `$name` member is declared by the shape, so the reference
                        // implementation reports the conflict where it is declared. A member with
                        // only applied traits is a copy of the first mixin's member.
                        let location = match self.elided.get(name) {
                            Some(location) => location.clone(),
                            None => previous.source.child("/target"),
                        };
                        self.diagnostics
                            .push(member_conflict(previous, location, mixin_member));
                        continue;
                    }
                    let error = self.error(
                        DiagnosticCode::InvalidShape,
                        format!(
                            "`{}` inherits member `{name}` with different targets: `{}` and `{}` \
                             (from `{}`)",
                            self.id(),
                            previous.target,
                            mixin_member.target,
                            mixin_member.id
                        ),
                        self.shape.source_location().child("/mixins"),
                    );
                    self.diagnostics.push(error);
                    return None;
                }
                if !has_own_traits {
                    previous.source = mixin_member.source.clone();
                }
            } else {
                let member = MemberShape::new(
                    member_id(self.id(), name),
                    mixin_member.target.clone(),
                    mixin_member.source.clone(),
                );
                members.insert(name.to_owned(), member);
                is_local.push(false);
            }
        }
        for (name, own) in local {
            if !members.contains_key(&name) {
                members.insert(name, own);
                is_local.push(true);
            }
        }
        Some(MemberLayout {
            members,
            is_local,
            inherited,
        })
    }

    /// Gives each inherited member its inherited traits, overridden by its own: the traits of the
    /// shape's redefinition, or else the traits held for it (applied to it, or written on its
    /// `$name` declaration).
    fn resolve_member_traits(&mut self, layout: MemberLayout<'_>) -> Members {
        let MemberLayout {
            mut members,
            is_local,
            mut inherited,
        } = layout;
        for ((name, member), is_local) in members.iter_mut().zip(is_local) {
            let Some(mut traits) = inherited.remove(name.as_str()) else {
                continue;
            };
            // A declared member may also have applied traits that another definition holds.
            let mut own = if is_local {
                std::mem::take(&mut member.traits)
            } else {
                TraitMap::new()
            };
            own.extend(self.held.own_traits(&member.id));
            let introduced = own.keys().cloned().collect();
            for (trait_id, value) in own {
                traits.insert(trait_id, value);
            }
            member.traits = traits;
            member.inherited = Some(Box::new(InheritedMember { introduced }));
            if let Some(location) = self.elided.get(name) {
                member.source = location.clone();
            }
        }
        members
    }

    /// Reports each `$name` member that no mixin provides. Like the reference implementation, the
    /// traits written on it are dropped with it, without a diagnostic.
    fn check_elided_members(&mut self, members: &Members) {
        for (name, location) in self.elided {
            if members.get(name).is_some_and(MemberShape::is_inherited) {
                continue;
            }
            let member = member_id(self.id(), name);
            self.held.discard(&member);
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::InvalidProperty,
                    format!(
                        "cannot elide the target of `${name}`: no bound resource or mixin of `{}` \
                         has an identifier, property, or member named `{name}`",
                        self.id()
                    ),
                )
                .with_shape_id(member)
                .with_source_location(location.clone()),
            );
        }
    }
}

/// A shape's traits: each mixin's, then its own, each overriding the earlier ones.
fn shape_traits(mixins: &[&Shape], own: TraitMap) -> TraitMap {
    let mut traits = TraitMap::new();
    for mixin in mixins {
        inherit_shape_traits(mixin, &mut traits);
    }
    for (trait_id, value) in own {
        traits.insert(trait_id, value);
    }
    traits
}

/// A member that keeps its target although a mixin member with its name has another one.
fn member_conflict(
    own: &MemberShape,
    location: SourceLocation,
    mixin_member: &MemberShape,
) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::InvalidShape,
        format!(
            "member `{}` conflicts with the inherited mixin member `{}`, which targets `{}`",
            own.id, mixin_member.id, mixin_member.target
        ),
    )
    .with_shape_id(own.id.clone())
    .with_source_location(location)
}

/// Adds the traits that `mixin` passes on: all of them, except its `@mixin` trait and the
/// `localTraits` it lists. A shape without `@mixin` passes on every trait (the `Target`
/// check reports using it as a mixin).
fn inherit_shape_traits(mixin: &Shape, traits: &mut TraitMap) {
    let common = mixin.common();
    let local = common.traits.get(&traits::MIXIN).map(local_traits);
    for (trait_id, value) in &common.traits {
        let is_local = local
            .as_ref()
            .is_some_and(|local| *trait_id == traits::MIXIN || local.contains(trait_id));
        if !is_local {
            traits.insert(trait_id.clone(), value.clone());
        }
    }
}

/// The `localTraits` of a `@mixin` trait value. Invalid entries are ignored here.
fn local_traits(value: &Node) -> HashSet<ShapeId> {
    value
        .as_object()
        .and_then(|object| object.get("localTraits"))
        .and_then(Node::as_array)
        .into_iter()
        .flatten()
        .filter_map(Node::as_str)
        .filter_map(|id| ShapeId::new(id).ok())
        .collect()
}
