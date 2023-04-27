/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Layered Configuration Bag Structure
//!
//! [`config_bag::ConfigBag`] and [`config_bag::FrozenConfigBag`] are the two representations of a layered configuration structure
//! with the following properties:
//! 1. A new layer of configuration may be applied onto an existing configuration structure without modifying it or taking ownership.
//! 2. No lifetime shenanigans to deal with
mod typeid_map;

use crate::config_bag::typeid_map::TypeIdMap;

use std::any::{type_name, Any, TypeId};
use std::collections::HashSet;

use std::fmt::{Debug, Formatter};
use std::iter::Rev;
use std::marker::PhantomData;
use std::ops::Deref;
use std::slice;
use std::sync::Arc;

/// Layered Configuration Structure
///
/// [`ConfigBag`] is the "unlocked" form of the bag. Only the top layer of the bag may be unlocked.
#[must_use]
pub struct ConfigBag {
    head: Layer,
    tail: Option<FrozenConfigBag>,
}

impl Debug for ConfigBag {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        struct Layers<'a>(&'a ConfigBag);
        impl Debug for Layers<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.debug_list().entries(self.0.layers()).finish()
            }
        }
        f.debug_struct("ConfigBag")
            .field("layers", &Layers(&self))
            .finish()
    }
}

/// Layered Configuration Structure
///
/// [`FrozenConfigBag`] is the "locked" form of the bag.
#[derive(Clone, Debug)]
#[must_use]
pub struct FrozenConfigBag(Arc<ConfigBag>);

impl Deref for FrozenConfigBag {
    type Target = ConfigBag;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) mod value {
    #[derive(Debug)]
    pub enum Value<T> {
        Set(T),
        ExplicitlyUnset(&'static str),
    }
}
use value::Value;

impl<T: Default> Default for Value<T> {
    fn default() -> Self {
        Self::Set(Default::default())
    }
}

struct DebugErased {
    field: Box<dyn Any + Send + Sync>,
    type_name: &'static str,
    debug: Box<dyn Fn(&DebugErased, &mut Formatter<'_>) -> std::fmt::Result + Send + Sync>,
}

impl Debug for DebugErased {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        (self.debug)(&self, f)
    }
}

impl DebugErased {
    fn new<T: Send + Sync + Debug + 'static>(value: T) -> Self {
        let debug = |value: &DebugErased, f: &mut Formatter<'_>| {
            Debug::fmt(
                value
                    .as_ref::<T>()
                    .expect(&format!("typechecked: {:?}", type_name::<T>())),
                f,
            )
        };
        let name = type_name::<T>();
        Self {
            field: Box::new(value),
            type_name: name,
            debug: Box::new(debug),
        }
    }

    fn as_ref<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.field.downcast_ref()
    }

    fn as_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.field.downcast_mut()
    }
}

pub struct Layer {
    name: &'static str,
    props: TypeIdMap<DebugErased>,
}

pub trait Store: Sized + Send + Sync + 'static {
    type ReturnedType<'a>: Send + Sync;
    type StoredType: Send + Sync + Debug;

    fn merge_iter(iter: ItemIter<'_, Self>) -> Self::ReturnedType<'_>;
}

#[non_exhaustive]
pub struct StoreReplace<U>(PhantomData<U>);
#[non_exhaustive]
pub struct StoreAppend<U>(PhantomData<U>);

pub trait Storable: Send + Sync + Debug + 'static {
    type Storer: Store;
}

impl<U: Send + Sync + Debug + 'static> Store for StoreReplace<U> {
    type ReturnedType<'a> = Option<&'a U>;
    type StoredType = Value<U>;

    fn merge_iter<'a>(iter: ItemIter<'a, Self>) -> Self::ReturnedType<'a> {
        for item in iter {
            return match item {
                Value::Set(item) => Some(item),
                Value::ExplicitlyUnset(_) => None,
            };
        }
        None
    }
}

impl<U: Send + Sync + Debug + 'static> Store for StoreAppend<U> {
    type ReturnedType<'a> = AppendItemIter<'a, U>;
    type StoredType = Value<Vec<U>>;

    fn merge_iter<'a>(iter: ItemIter<'a, Self>) -> Self::ReturnedType<'a> {
        AppendItemIter {
            inner: iter,
            cur: None,
        }
    }
}

pub struct AppendItemIter<'a, U> {
    inner: ItemIter<'a, StoreAppend<U>>,
    cur: Option<Rev<slice::Iter<'a, U>>>,
}

impl<'a, U: 'a> Iterator for AppendItemIter<'a, U>
where
    U: Send + Sync + Debug + 'static,
{
    type Item = &'a U;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(buf) = &mut self.cur {
            match buf.next() {
                Some(item) => return Some(item),
                None => self.cur = None,
            }
        }
        match self.inner.next() {
            None => None,
            Some(Value::Set(u)) => {
                self.cur = Some(u.iter().rev());
                self.next()
            }
            Some(Value::ExplicitlyUnset(_)) => return None,
        }
    }
}

impl Debug for Layer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        struct Items<'a>(&'a Layer);
        impl Debug for Items<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.debug_list().entries(self.0.props.values()).finish()
            }
        }
        f.debug_struct("Layer")
            .field("name", &self.name)
            .field("items", &Items(self))
            .finish()
    }
}

impl Layer {
    pub fn put<T: Store>(&mut self, value: T::StoredType) -> &mut Self {
        self.props
            .insert(TypeId::of::<T>(), DebugErased::new(value));
        self
    }

    pub fn get<T: Send + Sync + Store + 'static>(&self) -> Option<&T::StoredType> {
        self.props
            .get(&TypeId::of::<T>())
            .map(|t| t.as_ref().expect("typechecked"))
    }

    pub fn get_mut<T: Send + Sync + Store + 'static>(&mut self) -> Option<&mut T::StoredType> {
        self.props
            .get_mut(&TypeId::of::<T>())
            .map(|t| t.as_mut().expect("typechecked"))
    }

    pub fn get_mut_or_default<T: Send + Sync + Store + 'static>(&mut self) -> &mut T::StoredType
    where
        T::StoredType: Default,
    {
        self.props
            .entry(TypeId::of::<T>())
            .or_insert_with(|| DebugErased::new(T::StoredType::default()))
            .as_mut()
            .expect("typechecked")
    }

    pub fn len(&self) -> usize {
        self.props.len()
    }
}

pub trait Accessor {
    type Setter: Setter;
    fn config(&self) -> &ConfigBag;
}

pub trait Setter {
    fn config(&mut self) -> &mut ConfigBag;
}

fn no_op(_: &mut ConfigBag) {}

impl FrozenConfigBag {
    /// Attempts to convert this bag directly into a [`ConfigBag`] if no other references exist
    ///
    /// This allows modifying the top layer of the bag. [`Self::add_layer`] may be
    /// used to add a new layer to the bag.
    pub fn try_modify(self) -> Option<ConfigBag> {
        Arc::try_unwrap(self.0).ok()
    }

    /// Add a new layer to the config bag
    ///
    /// This is equivalent to calling [`Self::with_fn`] with a no-op function
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_runtime_api::config_bag::ConfigBag;
    /// fn add_more_config(bag: &mut ConfigBag) { /* ... */ }
    /// let bag = ConfigBag::base().with_fn("first layer", |_| { /* add a property */ });
    /// let mut bag = bag.add_layer("second layer");
    /// add_more_config(&mut bag);
    /// let bag = bag.freeze();
    /// ```
    pub fn add_layer(&self, name: &'static str) -> ConfigBag {
        self.with_fn(name, no_op)
    }

    /// Add more items to the config bag
    pub fn with_fn(&self, name: &'static str, next: impl Fn(&mut ConfigBag)) -> ConfigBag {
        let new_layer = Layer {
            name,
            props: Default::default(),
        };
        let mut bag = ConfigBag {
            head: new_layer,
            tail: Some(self.clone()),
        };
        next(&mut bag);
        bag
    }
}

impl ConfigBag {
    pub fn base() -> Self {
        ConfigBag {
            head: Layer {
                name: "base",
                props: Default::default(),
            },
            tail: None,
        }
    }

    pub fn store_put<T>(&mut self, item: T) -> &mut Self
    where
        T: Storable<Storer = StoreReplace<T>>,
    {
        self.head.put::<StoreReplace<T>>(Value::Set(item));
        self
    }

    pub fn store_or_unset<T>(&mut self, item: Option<T>) -> &mut Self
    where
        T: Storable<Storer = StoreReplace<T>>,
    {
        let item = match item {
            Some(item) => Value::Set(item),
            None => Value::ExplicitlyUnset(type_name::<T>()),
        };
        self.head.put::<StoreReplace<T>>(item);
        self
    }

    /// This can only be used for types that use Append storage
    /// ```
    /// use aws_smithy_runtime_api::config_bag::{ConfigBag, Storable, StoreAppend, StoreReplace};
    /// let mut bag = ConfigBag::base();
    /// #[derive(Debug, PartialEq, Eq)]
    /// struct Interceptor(&'static str);
    /// impl Storable for Interceptor {
    ///     type Storer = StoreAppend<Interceptor>;
    /// }
    ///
    /// bag.store_append(Interceptor("123"));
    /// bag.store_append(Interceptor("456"));
    ///
    /// assert_eq!(
    ///     bag.load::<Interceptor>(),
    ///     vec![&Interceptor("123"), &Interceptor("456")]
    /// );
    /// ```
    pub fn store_append<T>(&mut self, item: T) -> &mut Self
    where
        T: Storable<Storer = StoreAppend<T>>,
    {
        match self.head.get_mut_or_default::<StoreAppend<T>>() {
            Value::Set(list) => list.push(item),
            v @ Value::ExplicitlyUnset(_) => *v = Value::Set(vec![item]),
        }
        self
    }

    pub fn clear<T>(&mut self)
    where
        T: Storable<Storer = StoreAppend<T>>,
    {
        self.head
            .put::<StoreAppend<T>>(Value::ExplicitlyUnset(type_name::<T>()));
    }

    pub fn load<T: Storable>(&self) -> <T::Storer as Store>::ReturnedType<'_> {
        self.sourced_get::<T::Storer>()
    }

    /// Retrieve the value of type `T` from the bag if exists
    pub fn get<T: Send + Sync + Debug + 'static>(&self) -> Option<&T> {
        let out = self.sourced_get::<StoreReplace<T>>();
        out
    }

    /// Insert `value` into the bag
    pub fn put_legacy<T: Send + Sync + Debug + 'static>(&mut self, value: T) -> &mut Self {
        self.head.put::<StoreReplace<T>>(Value::Set(value));
        self
    }

    /// Remove `T` from this bag
    pub fn unset<T: Send + Sync + Debug + 'static>(&mut self) -> &mut Self {
        self.head
            .put::<StoreReplace<T>>(Value::ExplicitlyUnset(type_name::<T>()));
        self
    }

    /// Freeze this layer by wrapping it in an `Arc`
    ///
    /// This prevents further items from being added to this layer, but additional layers can be
    /// added to the bag.
    pub fn freeze(self) -> FrozenConfigBag {
        self.into()
    }

    /// Add another layer to this configuration bag
    ///
    /// Hint: If you want to re-use this layer, call `freeze` first.
    /// ```
    /// use aws_smithy_runtime_api::config_bag::ConfigBag;
    /// let bag = ConfigBag::base();
    /// let first_layer = bag.with_fn("a", |b: &mut ConfigBag| { b.put_legacy("a"); }).freeze();
    /// let second_layer = first_layer.with_fn("other", |b: &mut ConfigBag| { b.put_legacy(1i32); });
    /// // The number is only in the second layer
    /// assert_eq!(first_layer.get::<i32>(), None);
    /// assert_eq!(second_layer.get::<i32>(), Some(&1));
    ///
    /// // The string is in both layers
    /// assert_eq!(first_layer.get::<&'static str>(), Some(&"a"));
    /// assert_eq!(second_layer.get::<&'static str>(), Some(&"a"));
    /// ```
    pub fn with_fn(self, name: &'static str, next: impl Fn(&mut ConfigBag)) -> ConfigBag {
        self.freeze().with_fn(name, next)
    }

    pub fn add_layer(self, name: &'static str) -> ConfigBag {
        self.freeze().add_layer(name)
    }

    pub fn sourced_get<T: Store>(&self) -> T::ReturnedType<'_> {
        let stored_type_iter = ItemIter {
            inner: self.layers(),
            t: PhantomData::default(),
        };
        T::merge_iter(stored_type_iter)
    }

    fn layers(&self) -> BagIter<'_> {
        BagIter { bag: Some(self) }
    }
}

pub struct ItemIter<'a, T> {
    inner: BagIter<'a>,
    t: PhantomData<T>,
}
impl<'a, T: 'a> Iterator for ItemIter<'a, T>
where
    T: Store,
{
    type Item = &'a T::StoredType;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next() {
            Some(layer) => layer.get::<T>().or_else(|| self.next()),
            None => None,
        }
    }
}

/// Iterator over the layers of a config bag
struct BagIter<'a> {
    bag: Option<&'a ConfigBag>,
}

impl<'a> Iterator for BagIter<'a> {
    type Item = &'a Layer;

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.bag {
            Some(bag) => Some(&bag.head),
            None => None,
        };
        if let Some(bag) = &mut self.bag {
            self.bag = bag.tail.as_deref();
        }
        next
    }
}

impl From<ConfigBag> for FrozenConfigBag {
    fn from(bag: ConfigBag) -> Self {
        FrozenConfigBag(Arc::new(bag))
    }
}

#[derive(Debug)]
pub enum SourceInfo {
    Set { layer: &'static str, value: String },
    Unset { layer: &'static str },
    Inherit { layer: &'static str },
}

#[cfg(test)]
mod test {
    use super::ConfigBag;
    use crate::config_bag::{Storable, StoreAppend, StoreReplace};

    #[test]
    fn layered_property_bag() {
        #[derive(Debug)]
        struct Prop1;
        #[derive(Debug)]
        struct Prop2;
        let layer_a = |bag: &mut ConfigBag| {
            bag.put_legacy(Prop1);
        };

        let layer_b = |bag: &mut ConfigBag| {
            bag.put_legacy(Prop2);
        };

        #[derive(Debug)]
        struct Prop3;

        let mut base_bag = ConfigBag::base()
            .with_fn("a", layer_a)
            .with_fn("b", layer_b);
        base_bag.put_legacy(Prop3);
        assert!(base_bag.get::<Prop1>().is_some());

        #[derive(Debug)]
        struct Prop4;

        let layer_c = |bag: &mut ConfigBag| {
            bag.put_legacy(Prop4);
            bag.unset::<Prop3>();
        };

        let base_bag = base_bag.freeze();
        let final_bag = base_bag.with_fn("c", layer_c);

        assert!(final_bag.get::<Prop4>().is_some());
        assert!(base_bag.get::<Prop4>().is_none());
        assert!(final_bag.get::<Prop1>().is_some());
        assert!(final_bag.get::<Prop2>().is_some());
        // we unset prop3
        assert!(final_bag.get::<Prop3>().is_none());
        println!("{:#?}", final_bag);
    }

    #[test]
    fn config_bag() {
        let bag = ConfigBag::base();
        #[derive(Debug)]
        struct Region(&'static str);
        let bag = bag.with_fn("service config", |layer: &mut ConfigBag| {
            layer.put_legacy(Region("asdf"));
        });

        assert_eq!(bag.get::<Region>().unwrap().0, "asdf");

        #[derive(Debug)]
        struct SigningName(&'static str);
        let bag = bag.freeze();
        let operation_config = bag.with_fn("operation", |layer: &mut ConfigBag| {
            layer.put_legacy(SigningName("s3"));
        });

        assert!(bag.get::<SigningName>().is_none());
        assert_eq!(operation_config.get::<SigningName>().unwrap().0, "s3");

        let mut open_bag = operation_config.with_fn("my_custom_info", |_bag: &mut ConfigBag| {});
        open_bag.put_legacy("foo");

        assert_eq!(open_bag.layers().count(), 4);
    }

    #[test]
    fn store_append() {
        let mut bag = ConfigBag::base();
        #[derive(Debug, PartialEq, Eq)]
        struct Interceptor(&'static str);
        impl Storable for Interceptor {
            type Storer = StoreAppend<Interceptor>;
        }

        bag.clear::<Interceptor>();
        // you can only call store_append because interceptor is marked with a vec
        bag.store_append(Interceptor("123"));
        bag.store_append(Interceptor("456"));

        let mut bag = bag.add_layer("next");
        bag.store_append(Interceptor("789"));

        assert_eq!(
            bag.load::<Interceptor>().collect::<Vec<_>>(),
            vec![
                &Interceptor("789"),
                &Interceptor("456"),
                &Interceptor("123")
            ]
        );

        bag.clear::<Interceptor>();
        assert_eq!(bag.load::<Interceptor>().count(), 0);
    }
}
