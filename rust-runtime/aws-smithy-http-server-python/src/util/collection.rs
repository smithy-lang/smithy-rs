/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides Rust equivalents of [collections.abc] Python classes.
//!
//! Creating a custom container is achived in Python via extending a `collections.abc.*` class:
//! ```python
//! class MySeq(collections.abc.Sequence):
//!     def __getitem__(self, index):  ...  # Required abstract method
//!     def __len__(self):  ...             # Required abstract method
//! ```
//! You just need to implement required abstract methods and you get
//! extra mixin methods for free.
//!
//! Ideally we also want to just extend abstract base classes from Python but
//! it is not supported yet: <https://github.com/PyO3/pyo3/issues/991>.
//!
//! Until then, we are providing traits with the required methods and, macros that
//! takes those types that implement those traits and provides mixin methods for them.
//!
//! [collections.abc]: https://docs.python.org/3/library/collections.abc.html

use pyo3::PyResult;

/// Rust version of [collections.abc.MutableMapping].
///
/// [collections.abc.MutableMapping]: https://docs.python.org/3/library/collections.abc.html#collections.abc.MutableMapping
pub trait PyMutableMapping {
    type Key;
    type Value;

    fn len(&self) -> PyResult<usize>;
    fn contains(&self, key: Self::Key) -> PyResult<bool>;
    fn get(&self, key: Self::Key) -> PyResult<Option<Self::Value>>;
    fn set(&mut self, key: Self::Key, value: Self::Value) -> PyResult<()>;
    fn del(&mut self, key: Self::Key) -> PyResult<()>;

    // TODO(Perf): This methods should return iterators instead of `Vec`s.
    fn keys(&self) -> PyResult<Vec<Self::Key>>;
    fn values(&self) -> PyResult<Vec<Self::Value>>;
}

/// Macro that provides mixin methods of [collections.abc.MutableMapping] to the implementing type.
///
/// [collections.abc.MutableMapping]: https://docs.python.org/3/library/collections.abc.html#collections.abc.MutableMapping
#[macro_export]
macro_rules! mutable_mapping_pymethods {
    ($ty:ident, keys_iter: $keys_iter: ident) => {
        const _: fn() = || {
            fn assert_impl<T: PyMutableMapping>() {}
            assert_impl::<$ty>();
        };

        #[pyo3::pyclass]
        struct $keys_iter(std::vec::IntoIter<<$ty as PyMutableMapping>::Key>);

        #[pyo3::pymethods]
        impl $keys_iter {
            fn __next__(&mut self) -> Option<<$ty as PyMutableMapping>::Key> {
                self.0.next()
            }
        }

        #[pyo3::pymethods]
        impl $ty {
            // -- collections.abc.Sized

            fn __len__(&self) -> pyo3::PyResult<usize> {
                self.len()
            }

            // -- collections.abc.Container

            fn __contains__(&self, key: <$ty as PyMutableMapping>::Key) -> pyo3::PyResult<bool> {
                self.contains(key)
            }

            // -- collections.abc.Iterable

            /// Returns an iterator over the keys of the dictionary.
            /// NOTE: This method currently causes all keys to be cloned.
            fn __iter__(&self) -> pyo3::PyResult<$keys_iter> {
                Ok($keys_iter(self.keys()?.into_iter()))
            }

            // -- collections.abc.Mapping

            fn __getitem__(
                &self,
                key: <$ty as PyMutableMapping>::Key,
            ) -> pyo3::PyResult<Option<<$ty as PyMutableMapping>::Value>> {
                <$ty as PyMutableMapping>::get(&self, key)
            }

            #[pyo3(signature = (key, default=None))]
            fn get(
                &self,
                key: <$ty as PyMutableMapping>::Key,
                default: Option<<$ty as PyMutableMapping>::Value>,
            ) -> pyo3::PyResult<Option<<$ty as PyMutableMapping>::Value>> {
                Ok(<$ty as PyMutableMapping>::get(&self, key)?.or(default))
            }

            /// Returns keys of the dictionary.
            /// NOTE: This method currently causes all keys to be cloned.
            fn keys(&self) -> pyo3::PyResult<Vec<<$ty as PyMutableMapping>::Key>> {
                <$ty as PyMutableMapping>::keys(&self)
            }

            /// Returns values of the dictionary.
            /// NOTE: This method currently causes all values to be cloned.
            fn values(&self) -> pyo3::PyResult<Vec<<$ty as PyMutableMapping>::Value>> {
                <$ty as PyMutableMapping>::values(&self)
            }

            /// Returns items (key, value) of the dictionary.
            /// NOTE: This method currently causes all keys and values to be cloned.
            fn items(
                &self,
            ) -> pyo3::PyResult<
                Vec<(
                    <$ty as PyMutableMapping>::Key,
                    <$ty as PyMutableMapping>::Value,
                )>,
            > {
                Ok(self
                    .keys()?
                    .into_iter()
                    .zip(self.values()?.into_iter())
                    .collect())
            }

            // -- collections.abc.MutableMapping

            fn __setitem__(
                &mut self,
                key: <$ty as PyMutableMapping>::Key,
                value: <$ty as PyMutableMapping>::Value,
            ) -> pyo3::PyResult<()> {
                self.set(key, value)
            }

            fn __delitem__(&mut self, key: <$ty as PyMutableMapping>::Key) -> pyo3::PyResult<()> {
                self.del(key)
            }

            #[pyo3(signature = (key, default=None))]
            fn pop(
                &mut self,
                key: <$ty as PyMutableMapping>::Key,
                default: Option<<$ty as PyMutableMapping>::Value>,
            ) -> pyo3::PyResult<<$ty as PyMutableMapping>::Value> {
                let val = self.__getitem__(key.clone())?;
                match val {
                    Some(val) => {
                        self.del(key)?;
                        Ok(val)
                    }
                    None => {
                        default.ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("unknown key"))
                    }
                }
            }

            fn popitem(
                &mut self,
            ) -> pyo3::PyResult<(
                <$ty as PyMutableMapping>::Key,
                <$ty as PyMutableMapping>::Value,
            )> {
                let key = self
                    .keys()?
                    .iter()
                    .cloned()
                    .next()
                    .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("no key"))?;
                let value = self.pop(key.clone(), None)?;
                Ok((key, value))
            }

            fn clear(&mut self, py: pyo3::Python) -> pyo3::PyResult<()> {
                loop {
                    match self.popitem() {
                        Ok(_) => {}
                        Err(err) if err.is_instance_of::<pyo3::exceptions::PyKeyError>(py) => {
                            return Ok(())
                        }
                        Err(err) => return Err(err),
                    }
                }
            }

            #[pyo3(signature = (key, default=None))]
            fn setdefault(
                &mut self,
                key: <$ty as PyMutableMapping>::Key,
                default: Option<<$ty as PyMutableMapping>::Value>,
            ) -> pyo3::PyResult<Option<<$ty as PyMutableMapping>::Value>> {
                match self.__getitem__(key.clone())? {
                    Some(value) => Ok(Some(value)),
                    None => {
                        if let Some(value) = default.clone() {
                            self.set(key, value)?;
                        }
                        Ok(default)
                    }
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pyo3::{prelude::*, py_run};

    use super::*;

    #[pyclass(mapping)]
    struct Map(HashMap<String, String>);

    impl PyMutableMapping for Map {
        type Key = String;
        type Value = String;

        fn len(&self) -> PyResult<usize> {
            Ok(self.0.len())
        }

        fn contains(&self, key: Self::Key) -> PyResult<bool> {
            Ok(self.0.contains_key(&key))
        }

        fn keys(&self) -> PyResult<Vec<Self::Key>> {
            Ok(self.0.keys().cloned().collect())
        }

        fn values(&self) -> PyResult<Vec<Self::Value>> {
            Ok(self.0.values().cloned().collect())
        }

        fn get(&self, key: Self::Key) -> PyResult<Option<Self::Value>> {
            Ok(self.0.get(&key).cloned())
        }

        fn set(&mut self, key: Self::Key, value: Self::Value) -> PyResult<()> {
            self.0.insert(key, value);
            Ok(())
        }

        fn del(&mut self, key: Self::Key) -> PyResult<()> {
            self.0.remove(&key);
            Ok(())
        }
    }

    mutable_mapping_pymethods!(Map, keys_iter: MapKeys);

    #[test]
    fn mutable_mapping() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let map = Map({
            let mut hash_map = HashMap::new();
            hash_map.insert("foo".to_string(), "bar".to_string());
            hash_map.insert("baz".to_string(), "qux".to_string());
            hash_map
        });

        Python::with_gil(|py| {
            let map = Bound::new(py, map)?;
            py_run!(
                py,
                map,
                r#"
# collections.abc.Sized
assert len(map) == 2

# collections.abc.Container
assert "foo" in map
assert "foobar" not in map

# collections.abc.Iterable
elems = ["foo", "baz"]

for elem in map:
    assert elem in elems

it = iter(map)
assert next(it) in elems
assert next(it) in elems
try:
    next(it)
    assert False, "should stop iteration"
except StopIteration:
    pass

assert set(list(map)) == set(["foo", "baz"])

# collections.abc.Mapping
assert map["foo"] == "bar"
assert map.get("baz") == "qux"
assert map.get("foobar") == None
assert map.get("foobar", "default") == "default"

assert set(list(map.keys())) == set(["foo", "baz"])
assert set(list(map.values())) == set(["bar", "qux"])
assert set(list(map.items())) == set([("foo", "bar"), ("baz", "qux")])

# collections.abc.MutableMapping
map["foobar"] = "bazqux"
del map["foo"]

try:
    map.pop("not_exist")
    assert False, "should throw KeyError"
except KeyError:
    pass
assert map.pop("not_exist", "default") == "default"
assert map.pop("foobar") == "bazqux"
assert "foobar" not in map

# at this point there is only `baz => qux` in `map`
assert map.popitem() == ("baz", "qux")
assert len(map) == 0
try:
    map.popitem()
    assert False, "should throw KeyError"
except KeyError:
    pass

map["foo"] = "bar"
assert len(map) == 1
map.clear()
assert len(map) == 0
assert "foo" not in "bar"

assert map.setdefault("foo", "bar") == "bar"
assert map["foo"] == "bar"
assert map.setdefault("foo", "baz") == "bar"

# TODO(MissingImpl): Add tests for map.update(...)
"#
            );
            Ok(())
        })
    }
}
