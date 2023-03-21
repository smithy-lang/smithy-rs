/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::any::Any;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

#[derive(Debug)]
pub struct TypedBox<T> {
    inner: TypeErasedBox,
    _phantom: PhantomData<T>,
}

impl<T> TypedBox<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(inner: T) -> Self {
        Self {
            inner: TypeErasedBox::new(Box::new(inner) as _),
            _phantom: Default::default(),
        }
    }

    pub fn assume_from(type_erased: TypeErasedBox) -> Result<TypedBox<T>, TypeErasedBox> {
        if type_erased.downcast_ref::<T>().is_some() {
            Ok(TypedBox {
                inner: type_erased,
                _phantom: Default::default(),
            })
        } else {
            Err(type_erased)
        }
    }

    pub fn unwrap(self) -> T {
        *self.inner.downcast::<T>().expect("type checked")
    }

    pub fn erase(self) -> TypeErasedBox {
        self.inner
    }
}

impl<T: 'static> Deref for TypedBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.downcast_ref().expect("type checked")
    }
}

impl<T: 'static> DerefMut for TypedBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.downcast_mut().expect("type checked")
    }
}

#[derive(Debug)]
pub struct TypeErasedBox {
    inner: Box<dyn Any + Send + Sync>,
}

impl TypeErasedBox {
    pub fn new(inner: Box<dyn Any + Send + Sync>) -> Self {
        Self { inner }
    }

    pub fn downcast<T: 'static>(self) -> Result<Box<T>, Self> {
        match self.inner.downcast() {
            Ok(t) => Ok(t),
            Err(s) => Err(Self { inner: s }),
        }
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Foo(&'static str);
    #[derive(Debug)]
    struct Bar(isize);

    #[test]
    fn test() {
        let foo = TypedBox::new(Foo("1"));
        let bar = TypedBox::new(Bar(2));

        let mut foo_erased = foo.erase();
        foo_erased
            .downcast_mut::<Foo>()
            .expect("I know its a Foo")
            .0 = "3";

        let bar_erased = bar.erase();

        let bar_erased = TypedBox::<Foo>::assume_from(bar_erased).expect_err("it's not a Foo");
        let mut bar = TypedBox::<Bar>::assume_from(bar_erased).expect("it's a Bar");
        assert_eq!(2, bar.0);
        bar.0 += 1;

        let bar = bar.unwrap();
        assert_eq!(3, bar.0);

        assert!(foo_erased.downcast_ref::<Bar>().is_none());
        assert!(foo_erased.downcast_mut::<Bar>().is_none());
        let mut foo_erased = foo_erased.downcast::<Bar>().expect_err("it's not a Bar");

        assert_eq!("3", foo_erased.downcast_ref::<Foo>().expect("it's a Foo").0);
        foo_erased.downcast_mut::<Foo>().expect("it's a Foo").0 = "4";
        let foo = *foo_erased.downcast::<Foo>().expect("it's a Foo");
        assert_eq!("4", foo.0);
    }
}
