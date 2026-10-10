/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema extensions: derived data that runtime components compute once per
//! schema and cache on the schema itself.
//!
//! A codec, protocol, or any other runtime component declares a
//! [`SchemaExtensionKey`] as a `static`, pairing the type of the data with the
//! function that derives it from a schema. [`Schema::extension`] runs that
//! function the first time a given schema is asked for that key and returns the
//! cached value on every later call.
//!
//! ```
//! use aws_smithy_schema::extension::SchemaExtensionKey;
//! use aws_smithy_schema::{shape_id, Schema, ShapeType};
//!
//! /// A pre-encoded `"name":` JSON object key.
//! struct FieldKey(Box<str>);
//!
//! static FIELD_KEY: SchemaExtensionKey<FieldKey> = SchemaExtensionKey::new(|schema| {
//!     FieldKey(format!("\"{}\":", schema.member_name().unwrap_or_default()).into())
//! });
//!
//! static MEMBER: Schema<'static> =
//!     Schema::new_member(shape_id!("ns", "S", "name"), ShapeType::String, "name", 0);
//!
//! assert_eq!(&*MEMBER.extension(&FIELD_KEY).0, "\"name\":");
//! // The second lookup returns the same cached value.
//! assert!(std::ptr::eq(MEMBER.extension(&FIELD_KEY), MEMBER.extension(&FIELD_KEY)));
//! ```
//!
//! # Every schema has storage
//!
//! The storage is a field of [`Schema`], so a schema emitted by any version of the
//! code generator, a hand-written schema, and one materialized from a model at
//! runtime all cache identically. No component needs a fallback path for schemas
//! that lack storage.
//!
//! A schema declared as a `const` rather than a `static` is copied at each use, so
//! values cached on it are discarded with each copy. Declare schemas as `static`s.
//!
//! Nothing is allocated until a schema is first asked for an extension. That
//! allocates room for four values, and each cached value is boxed. The cache is
//! freed when the schema is dropped, so a schema built from a model at runtime
//! does not leak its cached values.
//!
//! # Providers
//!
//! A provider must only derive data from the schema it is given. In particular it
//! must not request the same key for the same schema, which would deadlock, and it
//! should not eagerly request extensions of other schemas, because a recursive shape
//! can lead back to the schema being initialized. Ask for a member's extension at
//! the point it is needed instead.
//!
//! Values must not depend on per-instance codec settings, because a schema is shared
//! by every codec instance in the process. Cache every variant a setting can select
//! (for example, both the `@jsonName` and the member name) and choose at the use
//! site.
//!
//! If a provider panics, the panic propagates to the caller and nothing is cached;
//! the next request for that key runs the provider again.
//!
//! Values are `'static`, so they cannot borrow from the schema. Store owned data,
//! or member indices rather than member schema references.

use crate::Schema;
use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// Identifies one kind of derived data cached on schemas, and computes it.
///
/// Declare keys as `static`s. Each key is assigned a process-unique slot the first
/// time it is used, and slots are never reused.
///
/// [`Schema::extension`] takes a `&'static` key, so a `const` key, which would be a
/// new key with a new slot at every use, is rejected at compile time:
///
/// ```compile_fail
/// use aws_smithy_schema::extension::SchemaExtensionKey;
/// use aws_smithy_schema::prelude;
///
/// const KEY: SchemaExtensionKey<u8> = SchemaExtensionKey::new(|_| 0);
/// let _ = prelude::STRING.extension(&KEY);
/// ```
pub struct SchemaExtensionKey<T> {
    /// `0` until assigned; afterwards the slot index plus one.
    slot: AtomicUsize,
    provider: fn(&Schema<'_>) -> T,
    _value: PhantomData<fn() -> T>,
}

impl<T> SchemaExtensionKey<T> {
    /// Creates a key whose values are derived by `provider`.
    pub const fn new(provider: fn(&Schema<'_>) -> T) -> Self {
        Self {
            slot: AtomicUsize::new(0),
            provider,
            _value: PhantomData,
        }
    }

    #[inline]
    fn slot(&self) -> usize {
        match self.slot.load(Ordering::Relaxed) {
            0 => self.assign_slot(),
            encoded => encoded - 1,
        }
    }

    #[cold]
    fn assign_slot(&self) -> usize {
        // The slot number is the only shared state, and the value cells it indexes
        // synchronize themselves, so relaxed ordering is enough. A thread that loses
        // the race discards its candidate; slots are plentiful, so the gap is harmless.
        static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
        let candidate = NEXT_SLOT.fetch_add(1, Ordering::Relaxed) + 1;
        match self
            .slot
            .compare_exchange(0, candidate, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => candidate - 1,
            Err(existing) => existing - 1,
        }
    }
}

impl<T> fmt::Debug for SchemaExtensionKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaExtensionKey")
            .field("value_type", &std::any::type_name::<T>())
            .finish_non_exhaustive()
    }
}

/// Number of value cells per chunk. A process normally uses one or two codecs, each
/// with one or two keys, so the first chunk is the only one most schemas allocate.
const CHUNK_LEN: usize = 4;

type Cell = OnceLock<Box<dyn Any + Send + Sync>>;

struct Chunk {
    cells: [Cell; CHUNK_LEN],
    next: OnceLock<Box<Chunk>>,
}

impl Chunk {
    fn new() -> Box<Self> {
        Box::new(Self {
            cells: [const { OnceLock::new() }; CHUNK_LEN],
            next: OnceLock::new(),
        })
    }
}

/// Per-schema extension storage. Empty until the schema is first asked for an
/// extension, so untouched schemas cost one word plus the cell's state.
pub(crate) struct ExtensionStorage(OnceLock<Box<Chunk>>);

impl ExtensionStorage {
    pub(crate) const fn new() -> Self {
        Self(OnceLock::new())
    }

    #[inline]
    pub(crate) fn get_or_compute<'s, T: Send + Sync + 'static>(
        &'s self,
        schema: &Schema<'_>,
        key: &SchemaExtensionKey<T>,
    ) -> &'s T {
        let mut slot = key.slot();
        let mut chunk: &Chunk = self.0.get_or_init(Chunk::new);
        while slot >= CHUNK_LEN {
            chunk = chunk.next.get_or_init(Chunk::new);
            slot -= CHUNK_LEN;
        }
        chunk.cells[slot]
            .get_or_init(|| Box::new((key.provider)(schema)))
            .downcast_ref::<T>()
            .expect("an extension slot is only ever filled by the key that owns it")
    }
}

impl fmt::Debug for ExtensionStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Cached values are derived data, so they are left out of `Schema`'s output.
        f.write_str("..")
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaExtensionKey;
    use crate::{shape_id, Schema, ShapeType};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    static LEN: SchemaExtensionKey<usize> = SchemaExtensionKey::new(|schema| {
        CALLS.fetch_add(1, Ordering::SeqCst);
        schema.member_name().map_or(0, str::len)
    });

    static MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("ns", "S", "abc"), ShapeType::String, "abc", 0);

    #[test]
    fn computes_once_per_schema() {
        let before = CALLS.load(Ordering::SeqCst);
        assert_eq!(*MEMBER.extension(&LEN), 3);
        assert_eq!(*MEMBER.extension(&LEN), 3);
        assert_eq!(CALLS.load(Ordering::SeqCst) - before, 1);
    }

    #[test]
    fn works_on_runtime_schemas() {
        let name = String::from("runtime");
        let id = crate::ShapeId::from_parts("ns#S$runtime", "ns", "S");
        let schema = Schema::new_member(id, ShapeType::String, &name, 0);
        assert_eq!(*schema.extension(&LEN), 7);
    }

    #[test]
    fn values_are_dropped_with_their_schema() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct Tracked;
        impl Drop for Tracked {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
        static KEY: SchemaExtensionKey<Tracked> = SchemaExtensionKey::new(|_| Tracked);

        let schema = Schema::new(shape_id!("ns", "Dropped"), ShapeType::String);
        let _ = schema.extension(&KEY);
        assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        drop(schema);
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_panicking_provider_caches_nothing() {
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        static KEY: SchemaExtensionKey<usize> = SchemaExtensionKey::new(|_| {
            if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("first attempt fails");
            }
            7
        });
        static S: Schema<'static> = Schema::new(shape_id!("ns", "Panics"), ShapeType::String);

        let first = std::panic::catch_unwind(|| *S.extension(&KEY));
        assert!(first.is_err());
        assert_eq!(*S.extension(&KEY), 7);
        assert_eq!(*S.extension(&KEY), 7);
        assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn many_keys_spill_into_later_chunks() {
        static KEYS: [SchemaExtensionKey<usize>; 9] = [
            SchemaExtensionKey::new(|_| 0),
            SchemaExtensionKey::new(|_| 1),
            SchemaExtensionKey::new(|_| 2),
            SchemaExtensionKey::new(|_| 3),
            SchemaExtensionKey::new(|_| 4),
            SchemaExtensionKey::new(|_| 5),
            SchemaExtensionKey::new(|_| 6),
            SchemaExtensionKey::new(|_| 7),
            SchemaExtensionKey::new(|_| 8),
        ];
        static S: Schema<'static> = Schema::new(shape_id!("ns", "T"), ShapeType::String);
        for (i, key) in KEYS.iter().enumerate() {
            assert_eq!(*S.extension(key), i);
        }
        for (i, key) in KEYS.iter().enumerate().rev() {
            assert_eq!(*S.extension(key), i);
        }
    }

    #[test]
    fn concurrent_first_access_computes_once() {
        static N: AtomicUsize = AtomicUsize::new(0);
        static KEY: SchemaExtensionKey<u64> = SchemaExtensionKey::new(|_| {
            N.fetch_add(1, Ordering::SeqCst);
            42
        });
        static S: Schema<'static> = Schema::new(shape_id!("ns", "U"), ShapeType::String);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert_eq!(*S.extension(&KEY), 42));
            }
        });
        assert_eq!(N.load(Ordering::SeqCst), 1);
    }
}
