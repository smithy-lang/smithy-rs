/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Abstractions for testing code that interacts with the operating system:
//! - Reading environment variables
//! - Reading from the file system

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::os_shim_internal::fs::Fake;

// Re-export the Env trait and types from the env module
pub use env::Env as EnvTrait;
pub use env::FakeEnv;
pub use env::RealEnv;
pub use env::SharedEnv;

/// Environment variable abstraction.
///
/// This type alias provides backward compatibility with existing code.
/// New code should use `SharedEnv` directly.
///
/// Environment variables are global to a process, and, as such, are difficult to test with a multi-
/// threaded test runner like Rust's. This enables loading environment variables either from the
/// actual process environment ([`std::env::var`]) or from a hash map.
///
/// Process environments are cheap to clone:
/// - Faked process environments are wrapped in an internal Arc
/// - Real process environments are pointer-sized
pub type Env = SharedEnv;

/// File system abstraction
///
/// Simple abstraction enabling in-memory mocking of the file system
///
/// # Examples
/// Construct a file system which delegates to `std::fs`:
/// ```rust
/// let fs = aws_types::os_shim_internal::Fs::real();
/// ```
///
/// Construct an in-memory file system for testing:
/// ```rust
/// use std::collections::HashMap;
/// let fs = aws_types::os_shim_internal::Fs::from_map({
///     let mut map = HashMap::new();
///     map.insert("/home/.aws/config".to_string(), "[default]\nregion = us-east-1");
///     map
/// });
/// ```
#[derive(Clone, Debug)]
pub struct Fs(fs::Inner);

impl Default for Fs {
    fn default() -> Self {
        Fs::real()
    }
}

impl Fs {
    /// Create `Fs` representing a real file system.
    pub fn real() -> Self {
        Fs(fs::Inner::Real)
    }

    /// Create `Fs` from a map of `OsString` to `Vec<u8>`.
    pub fn from_raw_map(fs: HashMap<OsString, Vec<u8>>) -> Self {
        Fs(fs::Inner::Fake(Arc::new(Fake::MapFs(Mutex::new(fs)))))
    }

    /// Create `Fs` from a map of `String` to `Vec<u8>`.
    pub fn from_map(data: HashMap<String, impl Into<Vec<u8>>>) -> Self {
        let fs = data
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        Self::from_raw_map(fs)
    }

    /// Create a test filesystem rooted in real files
    ///
    /// Creates a test filesystem from the contents of `test_directory` rooted into `namespaced_to`.
    ///
    /// Example:
    /// Given:
    /// ```bash
    /// $ ls
    /// ./my-test-dir/aws-config
    /// ./my-test-dir/aws-config/config
    /// $ cat ./my-test-dir/aws-config/config
    /// test-config
    /// ```
    /// ```rust,no_run
    /// # async fn docs() {
    /// use aws_types::os_shim_internal::{Env, Fs};
    /// let env = Env::from_slice(&[("HOME", "/Users/me")]);
    /// let fs = Fs::from_test_dir("my-test-dir/aws-config", "/Users/me/.aws/config");
    /// assert_eq!(fs.read_to_end("/Users/me/.aws/config").await.unwrap(), b"test-config");
    /// # }
    pub fn from_test_dir(
        test_directory: impl Into<PathBuf>,
        namespaced_to: impl Into<PathBuf>,
    ) -> Self {
        Self(fs::Inner::Fake(Arc::new(Fake::NamespacedFs {
            real_path: test_directory.into(),
            namespaced_to: namespaced_to.into(),
        })))
    }

    /// Create a fake process environment from a slice of tuples.
    ///
    /// # Examples
    /// ```rust
    /// # async fn example() {
    /// use aws_types::os_shim_internal::Fs;
    /// let mock_fs = Fs::from_slice(&[
    ///     ("config", "[default]\nretry_mode = \"standard\""),
    /// ]);
    /// assert_eq!(mock_fs.read_to_end("config").await.unwrap(), b"[default]\nretry_mode = \"standard\"");
    /// # }
    /// ```
    pub fn from_slice<'a>(files: &[(&'a str, &'a str)]) -> Self {
        let fs: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(k, v)| {
                let k = (*k).to_owned();
                let v = v.as_bytes().to_vec();
                (k, v)
            })
            .collect();

        Self::from_map(fs)
    }

    /// Read the entire contents of a file
    ///
    /// _Note: This function is currently `async` primarily for forward compatibility. Currently,
    /// this function does not use Tokio (or any other runtime) to perform IO, the IO is performed
    /// directly within the function._
    pub async fn read_to_end(&self, path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
        use fs::Inner;
        let path = path.as_ref();
        match &self.0 {
            // TODO(https://github.com/awslabs/aws-sdk-rust/issues/867): Use async IO below
            Inner::Real => std::fs::read(path),
            Inner::Fake(fake) => match fake.as_ref() {
                Fake::MapFs(fs) => fs
                    .lock()
                    .unwrap()
                    .get(path.as_os_str())
                    .cloned()
                    .ok_or_else(|| std::io::ErrorKind::NotFound.into()),
                Fake::NamespacedFs {
                    real_path,
                    namespaced_to,
                } => {
                    let actual_path = path
                        .strip_prefix(namespaced_to)
                        .map_err(|_| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                    std::fs::read(real_path.join(actual_path))
                }
            },
        }
    }

    /// Write a slice as the entire contents of a file.
    ///
    /// This is equivalent to `std::fs::write`.
    pub async fn write(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> std::io::Result<()> {
        use fs::Inner;
        match &self.0 {
            // TODO(https://github.com/awslabs/aws-sdk-rust/issues/867): Use async IO below
            Inner::Real => {
                std::fs::write(path, contents)?;
            }
            Inner::Fake(fake) => match fake.as_ref() {
                Fake::MapFs(fs) => {
                    fs.lock()
                        .unwrap()
                        .insert(path.as_ref().as_os_str().into(), contents.as_ref().to_vec());
                }
                Fake::NamespacedFs {
                    real_path,
                    namespaced_to,
                } => {
                    let actual_path = path
                        .as_ref()
                        .strip_prefix(namespaced_to)
                        .map_err(|_| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                    std::fs::write(real_path.join(actual_path), contents)?;
                }
            },
        }
        Ok(())
    }
}

mod fs {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    pub(super) enum Inner {
        Real,
        Fake(Arc<Fake>),
    }

    #[derive(Debug)]
    pub(super) enum Fake {
        MapFs(Mutex<HashMap<OsString, Vec<u8>>>),
        NamespacedFs {
            real_path: PathBuf,
            namespaced_to: PathBuf,
        },
    }
}

mod env {
    use std::collections::HashMap;
    use std::env::VarError;
    use std::fmt;
    use std::sync::Arc;

    /// Trait for accessing environment variables.
    ///
    /// This trait enables custom environment variable providers for testing,
    /// containerized environments, secrets managers, or other custom sources.
    ///
    /// # Thread Safety
    ///
    /// Implementations must be `Send + Sync` for thread-safe sharing across async contexts.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::env::VarError;
    /// use aws_types::os_shim_internal::EnvTrait;
    ///
    /// #[derive(Debug)]
    /// struct MyCustomEnv;
    ///
    /// impl EnvTrait for MyCustomEnv {
    ///     fn get(&self, key: &str) -> Result<String, VarError> {
    ///         // Custom implementation
    ///         Err(VarError::NotPresent)
    ///     }
    /// }
    /// ```
    pub trait Env: Send + Sync + fmt::Debug {
        /// Retrieve the value for the given key.
        ///
        /// Returns `VarError::NotPresent` if the key is not found.
        fn get(&self, key: &str) -> Result<String, VarError>;
    }

    /// Environment implementation that reads from the actual process environment.
    ///
    /// This is the default implementation used when no custom `Env` is provided.
    #[derive(Clone, Debug, Default)]
    pub struct RealEnv;

    impl Env for RealEnv {
        fn get(&self, key: &str) -> Result<String, VarError> {
            std::env::var(key)
        }
    }

    /// Environment implementation backed by a HashMap for testing.
    ///
    /// This implementation allows deterministic testing by providing
    /// controlled environment variable values.
    #[derive(Clone, Debug)]
    pub struct FakeEnv(Arc<HashMap<String, String>>);

    impl FakeEnv {
        /// Creates a `FakeEnv` from a slice of key-value pairs.
        ///
        /// # Examples
        /// ```rust
        /// use aws_types::os_shim_internal::{FakeEnv, EnvTrait};
        /// let env = FakeEnv::from_slice(&[
        ///     ("HOME", "/home/user"),
        ///     ("AWS_REGION", "us-west-2"),
        /// ]);
        /// // Note: FakeEnv requires EnvTrait import to call get()
        /// // For convenience, use SharedEnv which has an inherent get() method
        /// assert_eq!(env.get("HOME").unwrap(), "/home/user");
        /// ```
        pub fn from_slice(vars: &[(&str, &str)]) -> Self {
            let map: HashMap<String, String> = vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            Self(Arc::new(map))
        }
    }

    impl From<HashMap<String, String>> for FakeEnv {
        fn from(map: HashMap<String, String>) -> Self {
            Self(Arc::new(map))
        }
    }

    impl Env for FakeEnv {
        fn get(&self, key: &str) -> Result<String, VarError> {
            self.0.get(key).cloned().ok_or(VarError::NotPresent)
        }
    }

    /// A shared [`Env`] implementation.
    ///
    /// This wrapper enables sharing an environment provider across multiple
    /// components and threads. It implements `Env` by delegating to the
    /// wrapped implementation.
    ///
    /// # Examples
    /// ```rust
    /// use aws_types::os_shim_internal::{SharedEnv, FakeEnv};
    ///
    /// let fake = FakeEnv::from_slice(&[("HOME", "/home/user")]);
    /// let shared = SharedEnv::new(fake);
    /// assert_eq!(shared.get("HOME").unwrap(), "/home/user");
    /// ```
    #[derive(Clone)]
    pub struct SharedEnv(Arc<dyn Env>);

    impl SharedEnv {
        /// Creates a new `SharedEnv` from any `Env` implementation.
        pub fn new(env: impl Env + 'static) -> Self {
            Self(Arc::new(env))
        }

        /// Create a process environment that uses the real process environment.
        ///
        /// Calls will be delegated to [`std::env::var`].
        pub fn real() -> Self {
            Self::new(RealEnv)
        }

        /// Create a fake process environment from a slice of tuples.
        ///
        /// # Examples
        /// ```rust
        /// use aws_types::os_shim_internal::Env;
        /// let mock_env = Env::from_slice(&[
        ///     ("HOME", "/home/myname"),
        ///     ("AWS_REGION", "us-west-2")
        /// ]);
        /// assert_eq!(mock_env.get("HOME").unwrap(), "/home/myname");
        /// ```
        pub fn from_slice(vars: &[(&str, &str)]) -> Self {
            Self::new(FakeEnv::from_slice(vars))
        }

        /// Retrieve the value for the given key.
        ///
        /// Returns `VarError::NotPresent` if the key is not found.
        ///
        /// # Examples
        /// ```rust
        /// use aws_types::os_shim_internal::Env;
        /// let env = Env::from_slice(&[("HOME", "/home/user")]);
        /// assert_eq!(env.get("HOME").unwrap(), "/home/user");
        /// ```
        pub fn get(&self, key: &str) -> Result<String, VarError> {
            self.0.get(key)
        }
    }

    impl Default for SharedEnv {
        fn default() -> Self {
            Self::real()
        }
    }

    impl From<HashMap<String, String>> for SharedEnv {
        fn from(map: HashMap<String, String>) -> Self {
            Self::new(FakeEnv::from(map))
        }
    }

    impl fmt::Debug for SharedEnv {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SharedEnv").finish()
        }
    }

    impl Env for SharedEnv {
        fn get(&self, key: &str) -> Result<String, VarError> {
            self.0.get(key)
        }
    }

    aws_smithy_runtime_api::impl_shared_conversions!(convert SharedEnv from Env using SharedEnv::new);
}

#[cfg(test)]
mod test {
    use std::env::VarError;

    use crate::os_shim_internal::{Env, Fs};

    #[test]
    fn env_works() {
        let env = Env::from_slice(&[("FOO", "BAR")]);
        assert_eq!(env.get("FOO").unwrap(), "BAR");
        assert_eq!(
            env.get("OTHER").expect_err("no present"),
            VarError::NotPresent
        )
    }

    #[tokio::test]
    async fn fs_from_test_dir_works() {
        let fs = Fs::from_test_dir(".", "/users/test-data");
        let _ = fs
            .read_to_end("/users/test-data/Cargo.toml")
            .await
            .expect("file exists");

        let _ = fs
            .read_to_end("doesntexist")
            .await
            .expect_err("file doesnt exists");
    }

    #[tokio::test]
    async fn fs_round_trip_file_with_real() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test-file");

        let fs = Fs::real();
        fs.read_to_end(&path)
            .await
            .expect_err("file doesn't exist yet");

        fs.write(&path, b"test").await.expect("success");

        let result = fs.read_to_end(&path).await.expect("success");
        assert_eq!(b"test", &result[..]);
    }
}
