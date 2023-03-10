/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This code is functionally equivalent to `Extensions` in the `http` crate. Examples
// have been updated to be more relevant for smithy use, the interface has been made public,
// and the doc comments have been updated to reflect how the config bag is used in smithy-rs.
// Additionally, optimizations around the HTTP use case have been removed in favor or simpler code.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

type AnyMap = HashMap<TypeId, Box<dyn Any + Send + Sync>, BuildHasherDefault<IdHasher>>;

// With TypeIds as keys, there's no need to hash them. They are already hashes
// themselves, coming from the compiler. The IdHasher just holds the u64 of
// the TypeId, and then returns it, instead of doing any bit fiddling.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId calls write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }
}

/// A type-map of configuration data.
///
/// `ConfigBag` contains configuration use during a smithy client's request/response lifecycle.
#[derive(Default)]
pub struct ConfigBag {
    // In http where this property bag is usually empty, this makes sense. We will almost always put
    // something in the bag, so we could consider removing the layer of indirection.
    map: AnyMap,
}

impl ConfigBag {
    /// Create an empty `ConfigBag`.
    #[inline]
    pub fn new() -> ConfigBag {
        ConfigBag {
            map: AnyMap::default(),
        }
    }

    /// Insert a type into this `ConfigBag`.
    ///
    /// If a value of this type already existed, it will be returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # use aws_smithy_orchestrator::config_bag::ConfigBag;
    /// let mut props = ConfigBag::new();
    ///
    /// #[derive(Debug, Eq, PartialEq)]
    /// struct Endpoint(&'static str);
    /// assert!(props.insert(Endpoint("dynamo.amazon.com")).is_none());
    /// assert_eq!(
    ///     props.insert(Endpoint("kinesis.amazon.com")),
    ///     Some(Endpoint("dynamo.amazon.com"))
    /// );
    /// ```
    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(val))
            .and_then(|boxed| {
                (boxed as Box<dyn Any + 'static>)
                    .downcast()
                    .ok()
                    .map(|boxed| *boxed)
            })
    }

    /// Get a reference to a type previously inserted on this `ConfigBag`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use aws_smithy_orchestrator::config_bag::ConfigBag;
    /// let mut props = ConfigBag::new();
    /// assert!(props.get::<i32>().is_none());
    /// props.insert(5i32);
    ///
    /// assert_eq!(props.get::<i32>(), Some(&5i32));
    /// ```
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| (&**boxed as &(dyn Any + 'static)).downcast_ref())
    }

    /// Get a mutable reference to a type previously inserted on this `ConfigBag`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use aws_smithy_orchestrator::config_bag::ConfigBag;
    /// let mut props = ConfigBag::new();
    /// props.insert(String::from("Hello"));
    /// props.get_mut::<String>().unwrap().push_str(" World");
    ///
    /// assert_eq!(props.get::<String>().unwrap(), "Hello World");
    /// ```
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| (&mut **boxed as &mut (dyn Any + 'static)).downcast_mut())
    }

    /// Remove a type from this `ConfigBag`.
    ///
    /// If a value of this type existed, it will be returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # use aws_smithy_orchestrator::config_bag::ConfigBag;
    /// let mut props = ConfigBag::new();
    /// props.insert(5i32);
    /// assert_eq!(props.remove::<i32>(), Some(5i32));
    /// assert!(props.get::<i32>().is_none());
    /// ```
    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map.remove(&TypeId::of::<T>()).and_then(|boxed| {
            (boxed as Box<dyn Any + 'static>)
                .downcast()
                .ok()
                .map(|boxed| *boxed)
        })
    }

    /// Clear the `ConfigBag` of all inserted extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use aws_smithy_orchestrator::config_bag::ConfigBag;
    /// let mut props = ConfigBag::new();
    /// props.insert(5i32);
    /// props.clear();
    ///
    /// assert!(props.get::<i32>().is_none());
    #[inline]
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

impl fmt::Debug for ConfigBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigBag").finish()
    }
}

/// A wrapper of [`ConfigBag`] that can be safely shared across threads and cheaply cloned.
///
/// To access properties, use either `acquire` or `acquire_mut`. This can be one line for
/// single property accesses, for example:
/// ```rust
/// # use aws_smithy_orchestrator::config_bag::SharedConfigBag;
/// # let properties = SharedConfigBag::new();
/// let my_string = properties.acquire().get::<String>();
/// ```
///
/// For multiple accesses, the acquire result should be stored as a local since calling
/// acquire repeatedly will be slower than calling it once:
/// ```rust
/// # use aws_smithy_orchestrator::config_bag::SharedConfigBag;
/// # let properties = SharedConfigBag::new();
/// let props = properties.acquire();
/// let my_string = props.get::<String>();
/// let my_vec = props.get::<Vec<String>>();
/// ```
///
/// Use `acquire_mut` to insert properties into the bag:
/// ```rust
/// # use aws_smithy_orchestrator::config_bag::SharedConfigBag;
/// # let properties = SharedConfigBag::new();
/// properties.acquire_mut().insert("example".to_string());
/// ```
#[derive(Clone, Debug, Default)]
pub struct SharedConfigBag(Arc<Mutex<ConfigBag>>);

impl SharedConfigBag {
    /// Create an empty `SharedConfigBag`.
    pub fn new() -> Self {
        SharedConfigBag(Arc::new(Mutex::new(ConfigBag::new())))
    }

    /// Acquire an immutable reference to the property bag.
    pub fn acquire(&self) -> impl Deref<Target = ConfigBag> + '_ {
        self.0.lock().unwrap()
    }

    /// Acquire a mutable reference to the property bag.
    pub fn acquire_mut(&self) -> impl DerefMut<Target = ConfigBag> + '_ {
        self.0.lock().unwrap()
    }
}

impl From<ConfigBag> for SharedConfigBag {
    fn from(bag: ConfigBag) -> Self {
        SharedConfigBag(Arc::new(Mutex::new(bag)))
    }
}

#[cfg(test)]
#[test]
fn test_extensions() {
    #[derive(Debug, PartialEq)]
    struct MyType(i32);

    let mut extensions = ConfigBag::new();

    extensions.insert(5i32);
    extensions.insert(MyType(10));

    assert_eq!(extensions.get(), Some(&5i32));
    assert_eq!(extensions.get_mut(), Some(&mut 5i32));

    assert_eq!(extensions.remove::<i32>(), Some(5i32));
    assert!(extensions.get::<i32>().is_none());

    assert_eq!(extensions.get::<bool>(), None);
    assert_eq!(extensions.get(), Some(&MyType(10)));
}
