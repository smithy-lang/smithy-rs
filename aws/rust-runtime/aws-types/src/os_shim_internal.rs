/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Abstractions for testing code that interacts with the operating system:
//! - Reading environment variables
//! - Reading from the file system

// Re-export the Env trait and types from the env module
pub use env::Env;
pub use env::FakeEnv;
pub use env::RealEnv;
pub use env::SharedEnv;

// Re-export the Fs trait and types from the fs module
pub use fs::FakeFs;
pub use fs::Fs;
pub use fs::RealFs;
pub use fs::SharedFs;

mod fs {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fmt;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    /// Trait for file system operations.
    ///
    /// This trait enables custom file system providers for testing,
    /// containerized environments, or other custom sources.
    ///
    /// # Thread Safety
    ///
    /// Implementations must be `Send + Sync` for thread-safe sharing across async contexts.
    pub trait Fs: Send + Sync + fmt::Debug {
        /// Read the entire contents of a file.
        fn read_to_end(
            &self,
            path: &Path,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<Vec<u8>>> + Send + '_>,
        >;

        /// Write a slice as the entire contents of a file.
        fn write(
            &self,
            path: &Path,
            contents: &[u8],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + '_>>;
    }

    /// File system implementation that uses the real file system.
    #[derive(Clone, Debug, Default)]
    pub struct RealFs;

    impl Fs for RealFs {
        fn read_to_end(
            &self,
            path: &Path,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<Vec<u8>>> + Send + '_>,
        > {
            let path = path.to_owned();
            Box::pin(async move { std::fs::read(path) })
        }

        fn write(
            &self,
            path: &Path,
            contents: &[u8],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + '_>>
        {
            let path = path.to_owned();
            let contents = contents.to_vec();
            Box::pin(async move { std::fs::write(path, contents) })
        }
    }


    /// File system implementation backed by a HashMap for testing.
    #[derive(Clone, Debug)]
    pub struct FakeFs {
        data: Arc<Mutex<HashMap<OsString, Vec<u8>>>>,
        namespaced: Option<(PathBuf, PathBuf)>,
    }

    impl FakeFs {
        /// Create `FakeFs` from a map of `OsString` to `Vec<u8>`.
        pub fn from_raw_map(fs: HashMap<OsString, Vec<u8>>) -> Self {
            Self {
                data: Arc::new(Mutex::new(fs)),
                namespaced: None,
            }
        }

        /// Create `FakeFs` from a map of `String` to `Vec<u8>`.
        pub fn from_map(data: HashMap<String, impl Into<Vec<u8>>>) -> Self {
            let fs = data
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect();
            Self::from_raw_map(fs)
        }

        /// Create a test filesystem rooted in real files.
        pub fn from_test_dir(
            test_directory: impl Into<PathBuf>,
            namespaced_to: impl Into<PathBuf>,
        ) -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
                namespaced: Some((test_directory.into(), namespaced_to.into())),
            }
        }

        /// Create a fake file system from a slice of tuples.
        pub fn from_slice(files: &[(&str, &str)]) -> Self {
            let fs: HashMap<String, Vec<u8>> = files
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.as_bytes().to_vec()))
                .collect();
            Self::from_map(fs)
        }
    }

    impl Fs for FakeFs {
        fn read_to_end(
            &self,
            path: &Path,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<Vec<u8>>> + Send + '_>,
        > {
            let path = path.to_owned();
            let data = self.data.clone();
            let namespaced = self.namespaced.clone();
            Box::pin(async move {
                if let Some((real_path, namespaced_to)) = namespaced {
                    let actual_path = path
                        .strip_prefix(&namespaced_to)
                        .map_err(|_| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                    std::fs::read(real_path.join(actual_path))
                } else {
                    data.lock()
                        .unwrap()
                        .get(path.as_os_str())
                        .cloned()
                        .ok_or_else(|| std::io::ErrorKind::NotFound.into())
                }
            })
        }

        fn write(
            &self,
            path: &Path,
            contents: &[u8],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + '_>>
        {
            let path = path.to_owned();
            let contents = contents.to_vec();
            let data = self.data.clone();
            let namespaced = self.namespaced.clone();
            Box::pin(async move {
                if let Some((real_path, namespaced_to)) = namespaced {
                    let actual_path = path
                        .strip_prefix(&namespaced_to)
                        .map_err(|_| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                    std::fs::write(real_path.join(actual_path), contents)
                } else {
                    data.lock()
                        .unwrap()
                        .insert(path.as_os_str().into(), contents);
                    Ok(())
                }
            })
        }
    }


    /// A shared [`Fs`] implementation.
    #[derive(Clone)]
    pub struct SharedFs(Arc<dyn Fs>);

    impl SharedFs {
        /// Creates a new `SharedFs` from any `Fs` implementation.
        pub fn new(fs: impl Fs + 'static) -> Self {
            Self(Arc::new(fs))
        }

        /// Create `SharedFs` representing a real file system.
        pub fn real() -> Self {
            Self::new(RealFs)
        }

        /// Create `SharedFs` from a map of `OsString` to `Vec<u8>`.
        pub fn from_raw_map(fs: HashMap<OsString, Vec<u8>>) -> Self {
            Self::new(FakeFs::from_raw_map(fs))
        }

        /// Create `SharedFs` from a map of `String` to `Vec<u8>`.
        pub fn from_map(data: HashMap<String, impl Into<Vec<u8>>>) -> Self {
            Self::new(FakeFs::from_map(data))
        }

        /// Create a test filesystem rooted in real files.
        pub fn from_test_dir(
            test_directory: impl Into<PathBuf>,
            namespaced_to: impl Into<PathBuf>,
        ) -> Self {
            Self::new(FakeFs::from_test_dir(test_directory, namespaced_to))
        }

        /// Create a fake file system from a slice of tuples.
        pub fn from_slice(files: &[(&str, &str)]) -> Self {
            Self::new(FakeFs::from_slice(files))
        }
    }

    impl Default for SharedFs {
        fn default() -> Self {
            Self::real()
        }
    }

    impl fmt::Debug for SharedFs {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SharedFs").finish()
        }
    }

    impl Fs for SharedFs {
        fn read_to_end(
            &self,
            path: &Path,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<Vec<u8>>> + Send + '_>,
        > {
            self.0.read_to_end(path)
        }

        fn write(
            &self,
            path: &Path,
            contents: &[u8],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + '_>>
        {
            self.0.write(path, contents)
        }
    }

    aws_smithy_runtime_api::impl_shared_conversions!(convert SharedFs from Fs using SharedFs::new);
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
    /// use aws_types::os_shim_internal::Env;
    ///
    /// #[derive(Debug)]
    /// struct MyCustomEnv;
    ///
    /// impl Env for MyCustomEnv {
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
    #[derive(Clone, Debug, Default)]
    pub struct RealEnv;

    impl Env for RealEnv {
        fn get(&self, key: &str) -> Result<String, VarError> {
            std::env::var(key)
        }
    }

    /// Environment implementation backed by a HashMap for testing.
    #[derive(Clone, Debug)]
    pub struct FakeEnv(Arc<HashMap<String, String>>);

    impl FakeEnv {
        /// Creates a `FakeEnv` from a slice of key-value pairs.
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
    #[derive(Clone)]
    pub struct SharedEnv(Arc<dyn Env>);

    impl SharedEnv {
        /// Creates a new `SharedEnv` from any `Env` implementation.
        pub fn new(env: impl Env + 'static) -> Self {
            Self(Arc::new(env))
        }

        /// Create a process environment that uses the real process environment.
        pub fn real() -> Self {
            Self::new(RealEnv)
        }

        /// Create a fake process environment from a slice of tuples.
        pub fn from_slice(vars: &[(&str, &str)]) -> Self {
            Self::new(FakeEnv::from_slice(vars))
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

    use crate::os_shim_internal::{Env, Fs, SharedEnv, SharedFs};

    #[test]
    fn env_works() {
        let env = SharedEnv::from_slice(&[("FOO", "BAR")]);
        assert_eq!(env.get("FOO").unwrap(), "BAR");
        assert_eq!(
            env.get("OTHER").expect_err("no present"),
            VarError::NotPresent
        )
    }

    #[tokio::test]
    async fn fs_from_test_dir_works() {
        let fs = SharedFs::from_test_dir(".", "/users/test-data");
        let _ = fs
            .read_to_end("/users/test-data/Cargo.toml".as_ref())
            .await
            .expect("file exists");

        let _ = fs
            .read_to_end("doesntexist".as_ref())
            .await
            .expect_err("file doesnt exists");
    }

    #[tokio::test]
    async fn fs_round_trip_file_with_real() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test-file");

        let fs = SharedFs::real();
        fs.read_to_end(&path)
            .await
            .expect_err("file doesn't exist yet");

        fs.write(&path, b"test").await.expect("success");

        let result = fs.read_to_end(&path).await.expect("success");
        assert_eq!(b"test", &result[..]);
    }
}
