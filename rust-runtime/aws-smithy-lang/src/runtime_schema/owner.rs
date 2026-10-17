/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Stable-address storage for a runtime schema graph.
//!
//! This module contains the crate's only `unsafe` implementation. Schemas are built through
//! the safe [`Arena::build`] and [`Builder`], then read back through [`Storage`], whose two
//! accessors are `unsafe` because the storage no longer knows how long the borrowed data
//! lives. `RuntimeSchemaSet` is their only caller, relying on INV-4.
//!
//! # Why `unsafe` is needed
//!
//! `RuntimeSchemaSet` owns a `Model` and the schemas that borrow from it, so it is a
//! self-referential value. The requirements that force this are:
//!
//! 1. **Schemas borrow.** `Schema<'a>` holds `&'a str` for IDs, member names, and trait
//!    values, and `&'a Schema<'a>` for its members. That is the stable `aws-smithy-schema`
//!    representation shared with generated code, and it is what keeps generated schemas free
//!    of allocation and indirection on the request path. It is not ours to change.
//! 2. **The set is owned.** A dynamic client needs one value it can store, move, and share
//!    between threads (`Send + Sync`, no lifetime parameter), holding both the model and the
//!    schemas built from it.
//! 3. **It is reclaimable.** Dropping the set must free the schemas, so a process that reloads
//!    models does not leak memory.
//!
//! Safe Rust cannot express (2) given (1): a field cannot have a type that borrows from
//! another field of the same struct, because there is no lifetime naming "this value". The
//! only lifetimes available for the schemas' field are a parameter (the model then has to
//! live outside the set, violating 2) or `'static` (true only if the model and schemas are
//! leaked, violating 3). So the schemas are stored with their lifetime erased, and
//! [`Storage::schema`] and [`Storage::slice`] shorten it back to a borrow of the set. That
//! conversion is the one essential `unsafe` operation; everything else in this module
//! supports it.
//!
//! The safe alternatives each give up one requirement:
//!
//! - A caller-owned arena (`RuntimeSchemaSetRef<'a>`) is fully safe but gives up (2).
//! - Leaking the graph to get real `'static` schemas gives up (3); an EC2-sized graph is
//!   several MiB per load.
//! - Making `Schema` own its children through `Arc`s or indexes gives up (1): it changes a
//!   stable crate and adds size and indirection to every generated schema.
//! - A self-referential helper crate (`self_cell`, `ouroboros`, `yoke`) moves the same
//!   lifetime erasure into a dependency rather than removing it, and none supports a graph of
//!   many independently allocated values that reference each other without wrapping it in
//!   further types. A small, local, Miri-tested module was preferred to a new dependency;
//!   this can be revisited if one of those crates fits.
//!
//! Two further `unsafe` uses follow from how the storage is represented, not from the
//! self-reference itself:
//!
//! - **Raw pointers instead of `Box`.** The aliasing model that Miri checks treats moving a
//!   `Box` as an assertion that nothing else points into it, so pushing a `Box` into storage
//!   after taking a reference to its contents would invalidate that reference. Each value is
//!   therefore leaked to a `NonNull` immediately, which needs `unsafe` to turn back into a
//!   reference ([`Builder::alloc`]) and to free ([`Allocations`]' `Drop`). Safe typed arenas
//!   do the same internally.
//! - **`Send` and `Sync` for [`Storage`].** Raw pointers are neither, so the compiler cannot
//!   infer them; the reasoning is next to the `impl`s.
//!
//! # Invariants
//!
//! The numbered invariants are cited by every `unsafe` block below and by the tests that
//! exercise it (search for `INV-`).
//!
//! - **INV-1 stable addresses.** Every schema and member slice is allocated with `Box` and
//!   immediately converted to a raw pointer. The pointer, not the `Box`, is what moves when the
//!   storage vector grows or the owner moves, so pointees never move and moving the owner does
//!   not invalidate references into it.
//! - **INV-2 append-only.** No value is mutated, replaced, or freed after allocation until the
//!   storage itself is dropped.
//! - **INV-3 single release.** Each pointer is released exactly once, by the `Drop` of the
//!   [`Arena`] or [`Storage`] that owns it. [`Arena::build`] moves the pointers into the
//!   storage instead of dropping them.
//! - **INV-4 borrowed data outlives storage.** A [`Builder<'b>`] only accepts schemas whose
//!   data outlives `'b`, and `'b` is a fresh lifetime that the build closure must accept for
//!   any choice (it is invariant and higher-ranked). The only things known to outlive it are
//!   the `data` passed to [`Arena::build`], earlier allocations, and `'static` data, so the
//!   compiler rejects schemas that borrow anything else, such as a local (`E0597`), and no
//!   reference into the arena can be returned from the closure. Whoever reads from
//!   [`Storage`] must keep `data`'s borrowed contents alive and unmoved; `RuntimeSchemaSet`
//!   passes a `Model`, whose contents live in an `Arc` allocation that does not move with the
//!   `Model`, owns that `Model`, and drops the storage first.
//! - **INV-5 harmless drop.** Dropping a `Schema` only drops its extension cache, whose values
//!   are `'static`. It never reads the references it holds, so release order inside storage
//!   does not matter, and references may already dangle when the storage drops (for example,
//!   while unwinding from a panic in the builder).

use aws_smithy_schema::Schema;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::ptr::NonNull;

/// A schema whose data lifetime has been erased. Never dereferenced as `'static`; see
/// [`Storage::schema`].
type ErasedSchema = Schema<'static>;
type ErasedSlice = [&'static ErasedSchema];

/// Position of a schema in [`Storage`].
pub(super) type SchemaIndex = usize;
/// Position of a member slice in [`Storage`].
pub(super) type SliceIndex = usize;

/// Owns raw allocations and releases each exactly once.
struct Allocations {
    schemas: Vec<NonNull<ErasedSchema>>,
    slices: Vec<NonNull<ErasedSlice>>,
}

impl Drop for Allocations {
    fn drop(&mut self) {
        // Schemas first, then the slices that reference them (INV-5 makes either order sound).
        for schema in self.schemas.drain(..) {
            // SAFETY: INV-1 and INV-3: `schema` came from `Box::leak` in `Builder::alloc`
            // and is released only here. INV-5: dropping it reads no borrowed data.
            drop(unsafe { Box::from_raw(schema.as_ptr()) });
        }
        for slice in self.slices.drain(..) {
            // SAFETY: INV-1 and INV-3, as above, for `Builder::alloc_slice`. A slice of
            // references has no drop glue for its elements.
            drop(unsafe { Box::from_raw(slice.as_ptr()) });
        }
    }
}

/// Append-only storage used while a graph is being built.
pub(super) struct Arena {
    allocations: RefCell<Allocations>,
}

impl Arena {
    /// Runs `build` with a [`Builder`] whose schemas may borrow `data`, then freezes
    /// everything it allocated into a [`Storage`].
    ///
    /// If `build` fails or panics, everything it allocated is released (INV-3, INV-5).
    pub(super) fn build<T: ?Sized, R, E>(
        data: &T,
        build: impl for<'b> FnOnce(&'b T, Builder<'b>) -> Result<R, E>,
    ) -> Result<(R, Storage), E> {
        let arena = Arena {
            allocations: RefCell::new(Allocations {
                schemas: Vec::new(),
                slices: Vec::new(),
            }),
        };
        // `R` cannot name `'b`, so no reference into the arena survives this call.
        let result = build(
            data,
            Builder {
                arena: &arena,
                _brand: PhantomData,
            },
        )?;
        let mut allocations = arena.allocations.into_inner();
        allocations.schemas.shrink_to_fit();
        allocations.slices.shrink_to_fit();
        Ok((result, Storage { allocations }))
    }
}

/// Allocates schemas that live, and may borrow data that lives, for `'b`.
///
/// `'b` is invariant, so a builder cannot be converted to one with a shorter lifetime, which
/// would let a schema borrow a local (INV-4).
#[derive(Clone, Copy)]
pub(super) struct Builder<'b> {
    arena: &'b Arena,
    _brand: PhantomData<fn(&'b ()) -> &'b ()>,
}

impl<'b> Builder<'b> {
    /// Reserves room for `schemas` more schemas and `slices` more slices.
    pub(super) fn reserve(self, schemas: usize, slices: usize) {
        let mut allocations = self.arena.allocations.borrow_mut();
        allocations.schemas.reserve(schemas);
        allocations.slices.reserve(slices);
    }

    /// Moves `schema` to a stable address and returns its index and a reference to it.
    pub(super) fn alloc(self, schema: Schema<'b>) -> (SchemaIndex, &'b Schema<'b>) {
        let mut allocations = self.arena.allocations.borrow_mut();
        // Reserve before leaking the box so that `push` cannot panic and leak it.
        allocations.schemas.reserve(1);
        let pointer = NonNull::from(Box::leak(Box::new(schema)));
        let index = allocations.schemas.len();
        allocations.schemas.push(pointer.cast::<ErasedSchema>());
        drop(allocations);
        #[cfg(test)]
        tests::observe_allocation();
        // SAFETY: INV-1: `pointer` is a fresh, non-null `Box` allocation that never moves.
        // INV-2/INV-3: it is not freed until the arena drops or is frozen, neither of which
        // can happen while `'b` borrows it. Its data is borrowed for `'b`.
        (index, unsafe { pointer.as_ref() })
    }

    /// Moves `members` to a stable address and returns its index and a reference to it.
    pub(super) fn alloc_slice(
        self,
        members: Vec<&'b Schema<'b>>,
    ) -> (SliceIndex, &'b [&'b Schema<'b>]) {
        let mut allocations = self.arena.allocations.borrow_mut();
        allocations.slices.reserve(1);
        let pointer = NonNull::from(Box::leak(members.into_boxed_slice()));
        let index = allocations.slices.len();
        // The elements borrow the arena and their data for `'b`; both are erased.
        allocations
            .slices
            .push(NonNull::new(pointer.as_ptr() as *mut ErasedSlice).expect("non-null"));
        drop(allocations);
        #[cfg(test)]
        tests::observe_allocation();
        // SAFETY: INV-1, INV-2, INV-3 exactly as in `alloc`. The elements are references into
        // the arena, valid for `'b`.
        (index, unsafe { pointer.as_ref() })
    }
}

/// A frozen schema graph whose borrowed lifetime has been erased.
///
/// Created only by [`Arena::build`]. Nothing can be added or replaced (INV-2).
pub(super) struct Storage {
    allocations: Allocations,
}

// SAFETY: `Storage` exclusively owns its allocations (INV-3) and exposes them only as shared
// references (INV-2). `Schema` is `Send + Sync`, a slice of `&Schema` is therefore `Send +
// Sync`, and the data they borrow (model strings and other entries of this storage) is
// immutable and `Sync`.
unsafe impl Send for Storage {}
// SAFETY: as for `Send`; there is no interior mutability outside `Schema`'s own thread-safe
// extension cache.
unsafe impl Sync for Storage {}

impl Storage {
    /// The number of schemas.
    pub(super) fn schema_count(&self) -> usize {
        self.allocations.schemas.len()
    }

    /// The schema at `index`, with its lifetime shortened to `'a`.
    ///
    /// # Safety
    ///
    /// INV-4: the caller must ensure that what the `data` passed to [`Arena::build`] borrowed
    /// stays alive and unmoved for `'a`.
    ///
    /// # Panics
    ///
    /// If `index` was not returned by [`Builder::alloc`] for this storage.
    pub(super) unsafe fn schema<'a>(&'a self, index: SchemaIndex) -> &'a Schema<'a> {
        let pointer = self.allocations.schemas[index];
        // SAFETY: INV-1/INV-2/INV-3: the pointee is alive and unchanged while `self` is
        // borrowed. INV-4 (the caller's contract): the data it borrows outlives `'a`, so
        // shortening `Schema<'static>` to `Schema<'a>` (covariance) yields a valid reference.
        unsafe { pointer.as_ref() }
    }

    /// The member slice at `index`, with its lifetime shortened to `'a`.
    ///
    /// # Safety
    ///
    /// As for [`Storage::schema`].
    ///
    /// # Panics
    ///
    /// If `index` was not returned by [`Builder::alloc_slice`] for this storage.
    pub(super) unsafe fn slice<'a>(&'a self, index: SliceIndex) -> &'a [&'a Schema<'a>] {
        let pointer = self.allocations.slices[index];
        // SAFETY: as in `schema`; the elements point into this storage (INV-1).
        unsafe { pointer.as_ref() }
    }
}

#[cfg(test)]
pub(super) mod tests {
    //! Ownership tests. These are the Miri suite: run with
    //! `cargo +nightly miri test -p aws-smithy-lang --features runtime-schema --lib runtime_schema::`.

    use super::*;
    use aws_smithy_schema::extension::SchemaExtensionKey;
    use aws_smithy_schema::{ShapeId, ShapeType};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    thread_local! {
        /// Number of further allocations allowed before `observe_allocation` panics.
        static PANIC_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
        /// Allocations observed on this thread.
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    /// Called after every allocation; panics when an injected failure is due.
    pub(in crate::runtime_schema) fn observe_allocation() {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        PANIC_AFTER.with(|remaining| match remaining.get() {
            Some(0) => {
                remaining.set(None);
                panic!("injected allocation failure");
            }
            Some(n) => remaining.set(Some(n - 1)),
            None => {}
        });
    }

    /// Runs `f` with a panic injected after `allocations` successful allocations.
    pub(in crate::runtime_schema) fn with_panic_after<R>(
        allocations: usize,
        f: impl FnOnce() -> R,
    ) -> R {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                PANIC_AFTER.with(|remaining| remaining.set(None));
            }
        }
        let _reset = Reset;
        PANIC_AFTER.with(|remaining| remaining.set(Some(allocations)));
        f()
    }

    /// Counts allocations made by `f` on this thread.
    pub(in crate::runtime_schema) fn count_allocations(f: impl FnOnce()) -> usize {
        let before = ALLOCATIONS.with(Cell::get);
        f();
        ALLOCATIONS.with(Cell::get) - before
    }

    /// An extension value that counts its drops.
    pub(in crate::runtime_schema) struct Sentinel(
        pub(in crate::runtime_schema) &'static AtomicUsize,
    );
    impl Drop for Sentinel {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    static OWNER_DROPS: AtomicUsize = AtomicUsize::new(0);
    static OWNER_SENTINEL: SchemaExtensionKey<Sentinel> =
        SchemaExtensionKey::new(|_| Sentinel(&OWNER_DROPS));

    fn id<'a>(fqn: &'a str) -> ShapeId<'a> {
        let (namespace, name) = fqn.split_once('#').unwrap();
        ShapeId::from_parts(fqn, namespace, name)
    }

    /// Builds members that borrow `names`, plus a structure that references them.
    fn build_into<'b>(builder: Builder<'b>, names: &'b [String]) -> (SchemaIndex, SliceIndex) {
        let mut members = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let (_, member) = builder.alloc(Schema::new_member(
                id("smithy.api#String"),
                ShapeType::String,
                name,
                i,
            ));
            members.push(member);
        }
        let (slice, members) = builder.alloc_slice(members);
        let (root, _) = builder.alloc(Schema::new_struct(
            id("ns#Root"),
            ShapeType::Structure,
            members,
        ));
        (root, slice)
    }

    fn build(names: &[String]) -> ((SchemaIndex, SliceIndex), Storage) {
        Arena::build(names, |names, builder| {
            Ok::<_, ()>(build_into(builder, names))
        })
        .unwrap()
    }

    /// INV-1, INV-3, INV-4: borrows from moved storage stay valid while the borrowed data
    /// lives, and every schema is released once (Miri reports leaks and double frees).
    #[test]
    fn moved_storage_remains_readable() {
        let names: Box<[String]> = (0..3).map(|i| format!("m{i}")).collect();
        let ((root, slice), storage) = build(&names);
        let moved = Box::new(storage);
        let storage = *moved;
        // SAFETY: INV-4: `names` outlives `storage`.
        let root = unsafe { storage.schema(root) };
        // SAFETY: as above.
        let members = unsafe { storage.slice(slice) };
        assert_eq!(root.members().len(), 3);
        assert!(std::ptr::eq(root.members()[1], members[1]));
        assert_eq!(members[2].member_name(), Some("m2"));
        assert_eq!(storage.schema_count(), 4);
    }

    /// INV-5: extension values are dropped exactly once with their storage.
    #[test]
    fn extensions_drop_with_storage() {
        let names: Box<[String]> = (0..2).map(|i| format!("m{i}")).collect();
        let before = OWNER_DROPS.load(Ordering::SeqCst);
        for _ in 0..3 {
            let (_, storage) = build(&names);
            for index in 0..storage.schema_count() {
                // SAFETY: INV-4: `names` outlives `storage`.
                unsafe { storage.schema(index) }.extension(&OWNER_SENTINEL);
            }
            drop(storage);
        }
        assert_eq!(OWNER_DROPS.load(Ordering::SeqCst) - before, 9);
    }

    /// INV-3: a failed build releases everything it allocated.
    #[test]
    fn failed_construction_releases_allocations() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        static KEY: SchemaExtensionKey<Sentinel> = SchemaExtensionKey::new(|_| Sentinel(&DROPS));
        let names: Box<[String]> = (0..2).map(|i| format!("m{i}")).collect();
        let before = DROPS.load(Ordering::SeqCst);
        let result = Arena::build(&*names, |names, builder| {
            let (root, _) = build_into(builder, names);
            let _ = root;
            builder
                .alloc(Schema::new(id("ns#Extra"), ShapeType::String))
                .1
                .extension(&KEY);
            Err::<(), _>("failed")
        });
        assert!(matches!(result, Err("failed")));
        assert_eq!(DROPS.load(Ordering::SeqCst) - before, 1);
    }

    /// INV-3, INV-5: a panic during construction releases every allocation exactly once,
    /// including extensions forced before the panic, even though references between the
    /// released values dangle during the release.
    #[test]
    fn panicking_construction_releases_allocations() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        static KEY: SchemaExtensionKey<Sentinel> = SchemaExtensionKey::new(|_| Sentinel(&DROPS));
        let names: Box<[String]> = (0..3).map(|i| format!("m{i}")).collect();
        // 3 members, 1 slice, 1 root.
        for fail_after in 0..5 {
            let before = DROPS.load(Ordering::SeqCst);
            let forced = Cell::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_panic_after(fail_after, || {
                    Arena::build(&*names, |names, builder| {
                        let mut members = Vec::new();
                        for (i, name) in names.iter().enumerate() {
                            let (_, member) = builder.alloc(Schema::new_member(
                                id("smithy.api#String"),
                                ShapeType::String,
                                name,
                                i,
                            ));
                            member.extension(&KEY);
                            forced.set(forced.get() + 1);
                            members.push(member);
                        }
                        let (_, members) = builder.alloc_slice(members);
                        builder.alloc(Schema::new_struct(
                            id("ns#Root"),
                            ShapeType::Structure,
                            members,
                        ));
                        Ok::<_, ()>(())
                    })
                })
            }));
            assert!(result.is_err(), "allocation {fail_after} did not fail");
            // The panicking allocation was recorded before panicking, so it is released too,
            // but its extension was never forced.
            assert_eq!(DROPS.load(Ordering::SeqCst) - before, forced.get());
        }
    }
}
