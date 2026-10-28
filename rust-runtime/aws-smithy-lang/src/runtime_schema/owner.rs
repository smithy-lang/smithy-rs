/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Stable-address, append-only storage for a runtime schema graph.
//!
//! This module contains the crate's only `unsafe` implementation apart from the call sites of
//! its `unsafe` methods. [`Storage`] owns the data that schemas borrow (the model) and every
//! schema and member slice built from it. Schemas are built in batches with
//! [`Storage::with_builder`] and the safe [`Builder`], which returns borrow-free
//! [`SchemaHandle`]s and [`SliceHandle`]s. Batches may run after the storage is shared
//! between threads, so a lazily built set can add schemas while other threads read earlier
//! ones. Reading a handle back is `unsafe`, because the storage cannot check that a handle
//! is its own (INV-9).
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
//! [`Storage::schema`] and [`Storage::slice`] shorten it back to a borrow of the storage.
//! That conversion is the one essential `unsafe` operation; everything else in this module
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
//! Further `unsafe` uses follow from how the storage is represented, not from the
//! self-reference itself:
//!
//! - **Raw pointers instead of `Box`.** The aliasing model that Miri checks treats moving a
//!   `Box` as an assertion that nothing else points into it, so pushing a `Box` into storage
//!   after taking a reference to its contents would invalidate that reference. Each value is
//!   therefore leaked to a `NonNull` immediately, which needs `unsafe` to turn back into a
//!   reference and to free ([`Allocations`]' `Drop`). Safe typed arenas do the same
//!   internally.
//! - **Handles.** A later batch must refer to schemas built by an earlier one, and readers
//!   must not index a vector that a batch on another thread may be growing. Handles are the
//!   allocations' own pointers, so reading one touches only its immutable pointee. The
//!   higher-ranked builder proves that references made within one batch are valid; turning a
//!   handle from an earlier batch back into a reference ([`Builder::published_schema`],
//!   [`Storage::schema`], [`Storage::slice`]) relies on INV-9 instead. A later batch reaches
//!   an earlier slice through the schema that references it (`Schema::members`).
//! - **`Send` and `Sync`.** Raw pointers are neither, so the compiler cannot infer them for
//!   [`Storage`] or the handles; the reasoning is next to the `impl`s.
//!
//! # Invariants
//!
//! The numbered invariants are cited by every `unsafe` block below and by the tests that
//! exercise it (search for `INV-`).
//!
//! - **INV-1 stable addresses.** Every schema and member slice is allocated with `Box` and
//!   immediately converted to a raw pointer. The pointer, not the `Box`, is what moves when the
//!   allocation vectors grow or the storage moves, so pointees never move and moving the
//!   storage does not invalidate references into it.
//! - **INV-2 append-only.** Allocations may be added while the storage is shared, but no
//!   allocation is mutated, replaced, or freed until the storage itself is dropped. `Schema`'s
//!   own thread-safe extension cache is the only interior mutation.
//! - **INV-3 single release.** Each pointer is released exactly once, when the storage drops,
//!   including allocations of a batch that failed or panicked and were never published.
//! - **INV-4 borrowed data outlives the schemas.** A [`Builder<'b>`] only accepts schemas whose
//!   data outlives `'b`, and `'b` is a fresh lifetime that the batch closure must accept for
//!   any choice (it is invariant and higher-ranked). The only things known to outlive it are
//!   the storage's own data, earlier allocations, and `'static` data, so the compiler rejects
//!   schemas that borrow anything else, such as a local (`E0597`), and no reference branded
//!   with `'b` can be returned from the closure. The storage owns the data, never replaces
//!   it, and drops it after every schema; [`StableData`] guarantees that moving the storage
//!   does not move what the schemas borrow.
//! - **INV-5 harmless drop.** Dropping a `Schema` only drops its extension cache, whose values
//!   are `'static`. It never reads the references it holds, so release order inside storage
//!   does not matter, and references may already dangle when the storage drops.
//! - **INV-6 complete publication.** A handle is only returned once its pointee and everything
//!   the pointee references are fully built; callers publish handles (in `OnceLock`s) only
//!   after that.
//! - **INV-7 happens-before.** A handle read on another thread was published through a
//!   [`SchemaSlot`] (a release store read with an acquire load), a `OnceLock`, or the batch
//!   mutex, each of which orders the pointee's construction before the read. Readers never
//!   inspect the allocation vectors.
//! - **INV-8 serialized batches.** [`Storage::with_builder`] holds a mutex for the whole batch,
//!   so batches never interleave. The mutex is recovered after a panic, which is sound because
//!   a batch only appends allocations (INV-2, INV-3) and its caller only publishes complete
//!   values (INV-6).
//! - **INV-9 handle provenance.** A handle passed to [`Storage::schema`], [`Storage::slice`],
//!   or [`Builder::published_schema`] was returned by this same storage. A foreign handle would be undefined behavior rather than a wrong lookup, so
//!   debug builds tag every handle with its storage and check it.

use aws_smithy_schema::Schema;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// A schema whose data lifetime has been erased. Never dereferenced as `'static`; see
/// [`Storage::schema`].
type ErasedSchema = Schema<'static>;
type ErasedSlice = [&'static ErasedSchema];

/// Data that schemas may borrow while it is owned by a [`Storage`].
///
/// # Safety
///
/// Moving a value of the implementing type must not move or invalidate any string or schema
/// that can be borrowed through a shared reference to it. Data behind an `Arc` satisfies
/// this; data stored inline, or behind a `Box` (which the aliasing model treats as moved with
/// its owner), does not.
pub(super) unsafe trait StableData: Send + Sync {}

// SAFETY: a `Model` is an `Arc<ModelData>`; moving it moves only the pointer, and everything a
// schema borrows (IDs, member names, trait values) is inside the `Arc`'s allocation.
unsafe impl StableData for crate::Model {}

// SAFETY: as for `Model`: moving an `Arc` moves only the pointer to its allocation.
#[cfg(test)]
unsafe impl<T: ?Sized + Send + Sync> StableData for std::sync::Arc<T> {}

/// Identifies a storage in debug builds, so that a handle from another storage is caught
/// (INV-9). A counter rather than an address, so that a storage dropped and replaced at the
/// same address is still distinguished.
#[cfg(debug_assertions)]
type StorageTag = u64;

#[cfg(debug_assertions)]
fn next_tag() -> StorageTag {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A schema allocated by a [`Storage`]. Borrows nothing; see INV-9.
#[derive(Clone, Copy)]
pub(super) struct SchemaHandle {
    pointer: NonNull<ErasedSchema>,
    #[cfg(debug_assertions)]
    tag: StorageTag,
}

/// A member slice allocated by a [`Storage`]. Borrows nothing; see INV-9.
#[derive(Clone, Copy)]
pub(super) struct SliceHandle {
    pointer: NonNull<ErasedSlice>,
    #[cfg(debug_assertions)]
    tag: StorageTag,
}

// SAFETY: a handle is an inert address. It is only dereferenced through the `unsafe` methods
// of the storage that allocated it (INV-9), which hand out shared references to `Sync`
// values that are never mutated (INV-2) and whose construction happens before any read
// (INV-7).
unsafe impl Send for SchemaHandle {}
// SAFETY: as for `Send`.
unsafe impl Sync for SchemaHandle {}
// SAFETY: as for `SchemaHandle`.
unsafe impl Send for SliceHandle {}
// SAFETY: as for `SchemaHandle`.
unsafe impl Sync for SliceHandle {}

impl std::fmt::Debug for SchemaHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SchemaHandle").field(&self.pointer).finish()
    }
}

impl std::fmt::Debug for SliceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SliceHandle").field(&self.pointer).finish()
    }
}

/// A published schema handle: empty until filled once, then read without locking.
///
/// Like a `OnceLock<SchemaHandle>`, but one pointer wide and without `Once`'s
/// initialization protocol, which batches do not need because they are already serialized
/// (INV-8). Filling it is a release store and reading it an acquire load, so a reader that
/// sees the handle also sees the complete schema (INV-7).
pub(super) struct SchemaSlot {
    pointer: AtomicPtr<ErasedSchema>,
    #[cfg(debug_assertions)]
    tag: std::sync::atomic::AtomicU64,
}

impl SchemaSlot {
    pub(super) const fn new() -> Self {
        Self {
            pointer: AtomicPtr::new(std::ptr::null_mut()),
            #[cfg(debug_assertions)]
            tag: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// The handle, if published.
    pub(super) fn get(&self) -> Option<SchemaHandle> {
        let pointer = NonNull::new(self.pointer.load(Ordering::Acquire))?;
        Some(SchemaHandle {
            pointer,
            // Stored before the pointer's release store, so visible after its acquire load.
            #[cfg(debug_assertions)]
            tag: self.tag.load(Ordering::Relaxed),
        })
    }

    /// Publishes `handle`. Only call once the schema is complete (INV-6), from a batch, so
    /// that a slot is never filled twice (INV-8); filling it twice would only leave the
    /// first schema unused (INV-3).
    pub(super) fn set(&self, handle: SchemaHandle) {
        debug_assert!(self.get().is_none(), "a slot was filled twice");
        #[cfg(debug_assertions)]
        self.tag.store(handle.tag, Ordering::Relaxed);
        self.pointer
            .store(handle.pointer.as_ptr(), Ordering::Release);
    }
}

impl std::fmt::Debug for SchemaSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SchemaSlot").field(&self.get()).finish()
    }
}

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

/// Owns `data` and the schemas built from it, which may borrow it.
pub(super) struct Storage<T> {
    // Declared before `data` so that every schema is released before the data it borrows
    // (INV-4). The mutex serializes batches (INV-8); the `RefCell` gives the builder of the
    // batch that holds it mutable access.
    allocations: Mutex<RefCell<Allocations>>,
    /// The number of schemas, readable without waiting for a batch.
    schema_count: AtomicUsize,
    #[cfg(debug_assertions)]
    tag: StorageTag,
    data: T,
}

// SAFETY: `Storage` exclusively owns its allocations (INV-3) and exposes them only as shared
// references (INV-2). `Schema` is `Send + Sync`, a slice of `&Schema` is therefore `Send +
// Sync`, and the data they borrow (`T`, which is `Send + Sync`, and other allocations of this
// storage) is immutable. Allocation is serialized by the mutex (INV-8).
unsafe impl<T: StableData> Send for Storage<T> {}
// SAFETY: as for `Send`. Shared access only adds allocations, under the mutex, or reads
// published ones (INV-7); `Schema`'s extension cache is thread-safe.
unsafe impl<T: StableData> Sync for Storage<T> {}

impl<T: StableData> Storage<T> {
    /// Empty storage owning `data`.
    pub(super) fn new(data: T) -> Self {
        Self {
            allocations: Mutex::new(RefCell::new(Allocations {
                schemas: Vec::new(),
                slices: Vec::new(),
            })),
            schema_count: AtomicUsize::new(0),
            #[cfg(debug_assertions)]
            tag: next_tag(),
            data,
        }
    }

    /// The data that schemas borrow.
    pub(super) fn data(&self) -> &T {
        &self.data
    }

    /// The number of schemas allocated so far.
    pub(super) fn schema_count(&self) -> usize {
        self.schema_count.load(Ordering::Acquire)
    }

    /// Runs one batch: `build` receives the data and a [`Builder`] whose schemas may borrow it.
    ///
    /// Batches are serialized (INV-8), so this blocks while another thread runs one, and
    /// calling it again from inside `build` deadlocks. `R` cannot name `'b`, so only
    /// borrow-free values such as handles leave the batch. If `build` panics, its allocations
    /// stay owned by the storage (INV-3) and later batches run normally.
    pub(super) fn with_builder<R>(&self, build: impl for<'b> FnOnce(&'b T, Builder<'b>) -> R) -> R {
        let guard = self
            .allocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        build(
            &self.data,
            Builder {
                allocations: &guard,
                schema_count: &self.schema_count,
                #[cfg(debug_assertions)]
                tag: self.tag,
                _brand: PhantomData,
            },
        )
    }

    /// The schema `handle` refers to, with its lifetime shortened to `'a`.
    ///
    /// # Safety
    ///
    /// INV-9: `handle` must have been returned by a [`Builder`] of this storage.
    pub(super) unsafe fn schema<'a>(&'a self, handle: SchemaHandle) -> &'a Schema<'a> {
        #[cfg(debug_assertions)]
        assert_eq!(handle.tag, self.tag, "a schema handle from another storage");
        // SAFETY: INV-9 (the caller's contract): the pointee is an allocation of this storage,
        // so INV-1/INV-2/INV-3 keep it alive and unchanged while `self` is borrowed, and
        // INV-7 orders its construction before this read. INV-4: the data it borrows lives
        // as long as `self`, so shortening `Schema<'static>` to `Schema<'a>` (covariance)
        // yields a valid reference.
        unsafe { handle.pointer.as_ref() }
    }

    /// The member slice `handle` refers to, with its lifetime shortened to `'a`.
    ///
    /// # Safety
    ///
    /// As for [`Storage::schema`].
    pub(super) unsafe fn slice<'a>(&'a self, handle: SliceHandle) -> &'a [&'a Schema<'a>] {
        #[cfg(debug_assertions)]
        assert_eq!(handle.tag, self.tag, "a slice handle from another storage");
        // SAFETY: as in `schema`; the elements are allocations of this storage (INV-1).
        unsafe { handle.pointer.as_ref() }
    }
}

/// Allocates schemas that live, and may borrow data that lives, for `'b`.
///
/// `'b` is invariant, so a builder cannot be converted to one with a shorter lifetime, which
/// would let a schema borrow a local (INV-4).
#[derive(Clone, Copy)]
pub(super) struct Builder<'b> {
    allocations: &'b RefCell<Allocations>,
    schema_count: &'b AtomicUsize,
    #[cfg(debug_assertions)]
    tag: StorageTag,
    _brand: PhantomData<fn(&'b ()) -> &'b ()>,
}

impl<'b> Builder<'b> {
    /// Reserves room for `schemas` more schemas and `slices` more slices.
    pub(super) fn reserve(self, schemas: usize, slices: usize) {
        let mut allocations = self.allocations.borrow_mut();
        allocations.schemas.reserve(schemas);
        allocations.slices.reserve(slices);
    }

    /// Moves `schema` to a stable address and returns a handle and a reference to it.
    pub(super) fn alloc(self, schema: Schema<'b>) -> (SchemaHandle, &'b Schema<'b>) {
        let mut allocations = self.allocations.borrow_mut();
        // Reserve before leaking the box so that `push` cannot panic and leak it.
        allocations.schemas.reserve(1);
        let pointer = NonNull::from(Box::leak(Box::new(schema)));
        // Its data is borrowed for `'b`; the lifetime is erased (INV-4).
        let erased = pointer.cast::<ErasedSchema>();
        allocations.schemas.push(erased);
        // Batches are serialized, so a plain store suffices.
        self.schema_count
            .store(allocations.schemas.len(), Ordering::Release);
        drop(allocations);
        #[cfg(test)]
        tests::observe_allocation();
        let handle = SchemaHandle {
            pointer: erased,
            #[cfg(debug_assertions)]
            tag: self.tag,
        };
        // SAFETY: INV-1: `pointer` is a fresh, non-null `Box` allocation that never moves.
        // INV-2/INV-3: it is not freed until the storage drops, which cannot happen while `'b`
        // borrows it. Its data is borrowed for `'b`.
        (handle, unsafe { pointer.as_ref() })
    }

    /// Moves `members` to a stable address and returns a handle and a reference to it.
    pub(super) fn alloc_slice(
        self,
        members: Vec<&'b Schema<'b>>,
    ) -> (SliceHandle, &'b [&'b Schema<'b>]) {
        let mut allocations = self.allocations.borrow_mut();
        allocations.slices.reserve(1);
        let pointer = NonNull::from(Box::leak(members.into_boxed_slice()));
        // The elements borrow the storage and their data for `'b`; both are erased.
        let erased = NonNull::new(pointer.as_ptr() as *mut ErasedSlice).expect("non-null");
        allocations.slices.push(erased);
        drop(allocations);
        #[cfg(test)]
        tests::observe_allocation();
        let handle = SliceHandle {
            pointer: erased,
            #[cfg(debug_assertions)]
            tag: self.tag,
        };
        // SAFETY: INV-1, INV-2, INV-3 exactly as in `alloc`. The elements are references into
        // the storage, valid for `'b`.
        (handle, unsafe { pointer.as_ref() })
    }

    /// The schema `handle` refers to, for use in this batch's schemas.
    ///
    /// # Safety
    ///
    /// INV-9: `handle` must have been returned by a builder of the same storage, in this or
    /// an earlier batch.
    pub(super) unsafe fn published_schema(self, handle: SchemaHandle) -> &'b Schema<'b> {
        #[cfg(debug_assertions)]
        assert_eq!(handle.tag, self.tag, "a schema handle from another storage");
        // SAFETY: INV-9 (the caller's contract): the pointee is an allocation of this storage,
        // alive and unchanged until the storage drops (INV-1/INV-2/INV-3), which cannot
        // happen during this batch. INV-8 orders its construction before this batch. INV-4:
        // it borrows only the storage's data, earlier allocations, or `'static` data, all of
        // which outlive `'b`.
        unsafe { handle.pointer.as_ref() }
    }
}

#[cfg(test)]
pub(super) mod tests {
    //! Ownership tests. These are the Miri suite: run with
    //! `cargo +nightly miri test -p aws-smithy-lang --features runtime-schema --lib runtime_schema::`.
    //! CI runs these tests in this crate's `additional-ci` script.

    use super::*;
    use aws_smithy_schema::extension::SchemaExtensionKey;
    use aws_smithy_schema::{ShapeId, ShapeType};
    use std::cell::Cell;
    use std::sync::Arc;

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

    /// Runs `f` with a panic injected after `allocations` successful allocations on this
    /// thread.
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

    type Names = Arc<[String]>;

    fn names(count: usize) -> Names {
        (0..count).map(|i| format!("m{i}")).collect()
    }

    fn id<'a>(fqn: &'a str) -> ShapeId<'a> {
        let (namespace, name) = fqn.split_once('#').unwrap();
        ShapeId::from_parts(fqn, namespace, name)
    }

    /// Builds members that borrow `names`, plus a structure that references them.
    fn build_into<'b>(builder: Builder<'b>, names: &'b [String]) -> (SchemaHandle, SliceHandle) {
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

    fn build(names: Names) -> ((SchemaHandle, SliceHandle), Storage<Names>) {
        let storage = Storage::new(names);
        let handles = storage.with_builder(|names, builder| build_into(builder, names));
        (handles, storage)
    }

    /// INV-1, INV-3, INV-4, INV-9: handles stay valid when the storage moves, and every
    /// schema is released once (Miri reports leaks and double frees).
    #[test]
    fn moved_storage_remains_readable() {
        let ((root, slice), storage) = build(names(3));
        let moved = Box::new(storage);
        let storage = *moved;
        // SAFETY: INV-9: both handles come from `storage`.
        let root = unsafe { storage.schema(root) };
        // SAFETY: as above.
        let members = unsafe { storage.slice(slice) };
        assert_eq!(root.members().len(), 3);
        assert!(std::ptr::eq(root.members()[1], members[1]));
        assert_eq!(members[2].member_name(), Some("m2"));
        assert_eq!(storage.schema_count(), 4);
    }

    /// INV-2, INV-9: a later batch can reference schemas of an earlier one, and the slices
    /// they reference, and earlier references stay valid while the storage grows.
    #[test]
    fn later_batches_reference_earlier_ones() {
        let ((root, _), storage) = build(names(2));
        // SAFETY: INV-9: `root` comes from `storage`.
        let first = unsafe { storage.schema(root) };
        let (list, overlay) = storage.with_builder(|_, builder| {
            // Grow the allocation vectors well past their capacity.
            for _ in 0..64 {
                builder.alloc(Schema::new(id("ns#Filler"), ShapeType::String));
            }
            // SAFETY: INV-9: `root` comes from this storage's first batch.
            let root = unsafe { builder.published_schema(root) };
            let (list, _) = builder.alloc(Schema::new_list(id("ns#List"), root.members()[0]));
            let (overlay, _) = builder.alloc(Schema::new_struct(
                id("ns#Root"),
                ShapeType::Structure,
                root.members(),
            ));
            (list, overlay)
        });
        // SAFETY: INV-9: both handles come from `storage`.
        let (list, overlay) = unsafe { (storage.schema(list), storage.schema(overlay)) };
        assert!(std::ptr::eq(list.member().unwrap(), first.members()[0]));
        assert!(std::ptr::eq(overlay.members(), first.members()));
        assert_eq!(first.members()[1].member_name(), Some("m1"));
        // Two members and a root, then the batch's schemas.
        assert_eq!(storage.schema_count(), 3 + 64 + 2);
    }

    /// INV-2, INV-7, INV-8: one thread holds references while others append, and batches from
    /// several threads are serialized.
    #[test]
    fn readers_and_concurrent_batches() {
        let ((root, _), storage) = build(names(2));
        // SAFETY: INV-9: `root` comes from `storage`.
        let held = unsafe { storage.schema(root) };
        held.extension(&OWNER_SENTINEL);
        std::thread::scope(|scope| {
            for thread in 0..3 {
                let storage = &storage;
                scope.spawn(move || {
                    for _ in 0..4 {
                        let shape_type = if thread % 2 == 0 {
                            ShapeType::String
                        } else {
                            ShapeType::Integer
                        };
                        let handle = storage.with_builder(|_, builder| {
                            builder.alloc(Schema::new(id("ns#Thread"), shape_type)).0
                        });
                        // SAFETY: INV-9: `handle` comes from `storage`.
                        let schema = unsafe { storage.schema(handle) };
                        assert_eq!(schema.shape_id().as_str(), "ns#Thread");
                    }
                });
            }
            assert_eq!(held.members()[0].member_name(), Some("m0"));
        });
        assert_eq!(storage.schema_count(), 3 + 3 * 4);
        assert_eq!(held.members()[1].member_name(), Some("m1"));
    }

    /// INV-5: extension values are dropped exactly once with their storage.
    #[test]
    fn extensions_drop_with_storage() {
        let before = OWNER_DROPS.load(Ordering::SeqCst);
        for _ in 0..3 {
            let ((root, _), storage) = build(names(2));
            // SAFETY: INV-9: `root` comes from `storage`.
            let root = unsafe { storage.schema(root) };
            root.extension(&OWNER_SENTINEL);
            for member in root.members() {
                member.extension(&OWNER_SENTINEL);
            }
            // A later batch does not disturb forced extensions.
            storage.with_builder(|_, builder| {
                builder
                    .alloc(Schema::new(id("ns#Later"), ShapeType::String))
                    .0
            });
            drop(storage);
        }
        assert_eq!(OWNER_DROPS.load(Ordering::SeqCst) - before, 9);
    }

    /// INV-3, INV-5, INV-8: a panic during a batch keeps every allocation owned, releases each
    /// once with the storage, including extensions forced before the panic, and later batches
    /// still run.
    #[test]
    fn panicking_batches_keep_their_allocations() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        static KEY: SchemaExtensionKey<Sentinel> = SchemaExtensionKey::new(|_| Sentinel(&DROPS));
        // 3 members, 1 slice, 1 root.
        for fail_after in 0..5 {
            let before = DROPS.load(Ordering::SeqCst);
            let storage = Storage::new(names(3));
            let forced = Cell::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_panic_after(fail_after, || {
                    storage.with_builder(|names, builder| {
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
                    })
                })
            }));
            assert!(result.is_err(), "allocation {fail_after} did not fail");
            // The poisoned batch mutex is recovered.
            storage.with_builder(|names, builder| {
                build_into(builder, names);
            });
            drop(storage);
            // The panicking allocation was recorded before panicking, so it is released too,
            // but its extension was never forced.
            assert_eq!(DROPS.load(Ordering::SeqCst) - before, forced.get());
        }
    }

    /// INV-9: debug builds reject a handle from another storage.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a schema handle from another storage")]
    fn foreign_handles_are_rejected() {
        let ((root, _), _first) = build(names(1));
        let (_, second) = build(names(1));
        // SAFETY: not sound in release builds; the debug check panics before dereferencing.
        let _ = unsafe { second.schema(root) };
    }
}
