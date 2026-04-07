/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Abstractions for testing code that interacts with the operating system:
//! - Reading environment variables
//! - Reading from the file system
//! - Executing external processes

use std::collections::HashMap;
use std::env::VarError;
use std::ffi::OsString;
use std::fmt::{self, Debug};
use std::future::Future;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::os_shim_internal::fs::Fake;

/// Trait for custom environment variable providers.
pub trait ProvideEnv: Debug + Send + Sync + UnwindSafe + RefUnwindSafe {
    /// Get the value of environment variable `k`.
    fn get(&self, k: &str) -> Result<String, VarError>;
}

/// Trait for custom filesystem providers.
pub trait ProvideFs: Debug + Send + Sync + UnwindSafe + RefUnwindSafe {
    /// Read the entire contents of the file at `path`.
    fn read_to_end(
        &self,
        path: &Path,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<u8>>> + Send + '_>>;

    /// Write `contents` to the file at `path`.
    fn write(
        &self,
        path: &Path,
        contents: &[u8],
    ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>>;
}

/// Represents the exit status of a completed process.
///
/// Unlike [`std::process::ExitStatus`], this type can be constructed directly,
/// making it usable in tests and custom implementations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    code: Option<i32>,
}

impl ExitStatus {
    /// Create an `ExitStatus` from an exit code.
    ///
    /// A code of `0` represents success. `None` indicates the process
    /// was terminated by a signal (Unix) or otherwise didn't return a code.
    pub fn new(code: Option<i32>) -> Self {
        Self { code }
    }

    /// Returns `true` if the exit code was `0`.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// Returns the exit code, if available.
    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "exit status: {code}"),
            None => write!(f, "terminated by signal"),
        }
    }
}

impl From<std::process::ExitStatus> for ExitStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        Self {
            code: status.code(),
        }
    }
}

/// The output of a completed process, analogous to [`std::process::Output`].
#[derive(Clone, Debug)]
pub struct CommandOutput {
    /// The exit status of the process.
    pub status: ExitStatus,
    /// The data written to stdout.
    pub stdout: Vec<u8>,
    /// The data written to stderr.
    pub stderr: Vec<u8>,
}

/// Trait for custom process execution providers.
pub trait ProvideProcess: Debug + Send + Sync + UnwindSafe + RefUnwindSafe {
    /// Execute the given command string and return its output.
    ///
    /// The implementor is responsible for any shell wrapping or command
    /// interpretation needed. The built-in `Real` implementation wraps
    /// commands with `sh -c` (Unix) or `cmd.exe /C` (Windows).
    fn execute(
        &self,
        command: &str,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<CommandOutput>> + Send + '_>>;
}

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

    /// Create an `Fs` backed by a custom `ProvideFs` implementation.
    pub fn from_custom(provider: impl ProvideFs + 'static) -> Self {
        Self(fs::Inner::Custom(Arc::new(provider)))
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
            Inner::Custom(provider) => provider.read_to_end(path).await,
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
                std::fs::write(&path, contents)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
                }
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
            Inner::Custom(provider) => {
                return provider.write(path.as_ref(), contents.as_ref()).await
            }
        }
        Ok(())
    }
}

mod fs {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::ProvideFs;

    #[derive(Clone, Debug)]
    pub(super) enum Inner {
        Real,
        Fake(Arc<Fake>),
        Custom(Arc<dyn ProvideFs>),
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

/// Environment variable abstraction
///
/// Environment variables are global to a process, and, as such, are difficult to test with a multi-
/// threaded test runner like Rust's. This enables loading environment variables either from the
/// actual process environment ([`std::env::var`]) or from a hash map.
///
/// Process environments are cheap to clone:
/// - Faked process environments are wrapped in an internal Arc
/// - Real process environments are pointer-sized
#[derive(Clone, Debug)]
pub struct Env(env::Inner);

impl Default for Env {
    fn default() -> Self {
        Self::real()
    }
}

impl Env {
    /// Retrieve a value for the given `k` and return `VarError` is that key is not present.
    pub fn get(&self, k: &str) -> Result<String, VarError> {
        use env::Inner;
        match &self.0 {
            Inner::Real => std::env::var(k),
            Inner::Fake(map) => map.get(k).cloned().ok_or(VarError::NotPresent),
            Inner::Custom(provider) => provider.get(k),
        }
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
    pub fn from_slice<'a>(vars: &[(&'a str, &'a str)]) -> Self {
        let map: HashMap<_, _> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Self::from(map)
    }

    /// Create a process environment that uses the real process environment
    ///
    /// Calls will be delegated to [`std::env::var`].
    pub fn real() -> Self {
        Self(env::Inner::Real)
    }

    /// Create an `Env` backed by a custom `ProvideEnv` implementation.
    pub fn from_custom(provider: impl ProvideEnv + 'static) -> Self {
        Self(env::Inner::Custom(Arc::new(provider)))
    }
}

impl From<HashMap<String, String>> for Env {
    fn from(hash_map: HashMap<String, String>) -> Self {
        Self(env::Inner::Fake(Arc::new(hash_map)))
    }
}

mod env {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::ProvideEnv;

    #[derive(Clone, Debug)]
    pub(super) enum Inner {
        Real,
        Fake(Arc<HashMap<String, String>>),
        Custom(Arc<dyn ProvideEnv>),
    }
}

/// Process execution abstraction
///
/// Simple abstraction enabling in-memory mocking of process execution
///
/// # Examples
/// Construct a process executor that delegates to the OS:
/// ```rust
/// let process = aws_types::os_shim_internal::Process::real();
/// ```
///
/// Construct a fake process executor for testing:
/// ```rust
/// use aws_types::os_shim_internal::{Process, CommandOutput, ExitStatus};
/// let process = Process::from_slice(&[
///     ("echo hello", CommandOutput {
///         status: ExitStatus::new(Some(0)),
///         stdout: b"hello".to_vec(),
///         stderr: Vec::new(),
///     }),
/// ]);
/// ```
#[derive(Clone, Debug)]
pub struct Process(process::Inner);

impl Default for Process {
    fn default() -> Self {
        Process::real()
    }
}

impl Process {
    /// Create a `Process` that executes real OS processes.
    ///
    /// Commands are wrapped with the platform-specific shell:
    /// - Unix: `sh -c "<command>"`
    /// - Windows: `cmd.exe /C "<command>"`
    ///
    /// _Note: This function currently uses blocking `std::process::Command`
    /// for consistency with the `Fs` shim approach._
    pub fn real() -> Self {
        Process(process::Inner::Real)
    }

    /// Create a `Process` backed by a custom [`ProvideProcess`] implementation.
    pub fn from_custom(provider: impl ProvideProcess + 'static) -> Self {
        Self(process::Inner::Custom(Arc::new(provider)))
    }

    /// Create a fake `Process` from a slice of `(command, result)` pairs.
    ///
    /// When `execute` is called, the command string is looked up in the map.
    /// If not found, an `io::ErrorKind::NotFound` error is returned.
    ///
    /// # Examples
    /// ```rust
    /// use aws_types::os_shim_internal::{Process, CommandOutput, ExitStatus};
    /// let process = Process::from_slice(&[
    ///     ("echo hello", CommandOutput {
    ///         status: ExitStatus::new(Some(0)),
    ///         stdout: b"hello\n".to_vec(),
    ///         stderr: Vec::new(),
    ///     }),
    /// ]);
    /// ```
    pub fn from_slice(commands: &[(&str, CommandOutput)]) -> Self {
        let map: HashMap<String, CommandOutput> = commands
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Self(process::Inner::Fake(Arc::new(map)))
    }

    /// Execute a command string and return its output.
    ///
    /// For the `Real` variant, the command is wrapped in a platform-specific
    /// shell (`sh -c` on Unix, `cmd.exe /C` on Windows).
    ///
    /// _Note: The `Real` variant uses blocking I/O. For custom implementations,
    /// consider using [`Process::from_custom`] with a truly async provider._
    pub async fn execute(&self, command: &str) -> std::io::Result<CommandOutput> {
        use process::Inner;
        match &self.0 {
            Inner::Real => {
                let mut std_command = if cfg!(windows) {
                    let mut cmd = std::process::Command::new("cmd.exe");
                    cmd.args(["/C", command]);
                    cmd
                } else {
                    let mut cmd = std::process::Command::new("sh");
                    cmd.args(["-c", command]);
                    cmd
                };
                let output = std_command.output()?;
                Ok(CommandOutput {
                    status: ExitStatus::from(output.status),
                    stdout: output.stdout,
                    stderr: output.stderr,
                })
            }
            Inner::Fake(map) => map.get(command).cloned().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("command not found in fake process: {command}"),
                )
            }),
            Inner::Custom(provider) => provider.execute(command).await,
        }
    }
}

mod process {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::ProvideProcess;

    #[derive(Clone, Debug)]
    pub(super) enum Inner {
        Real,
        Fake(Arc<HashMap<String, super::CommandOutput>>),
        Custom(Arc<dyn ProvideProcess>),
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::env::VarError;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::Mutex;

    use crate::os_shim_internal::{
        CommandOutput, Env, ExitStatus, Fs, Process, ProvideEnv, ProvideFs, ProvideProcess,
    };

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

    #[test]
    fn custom_env_works() {
        #[derive(Debug)]
        struct CustomEnvProvider {
            vars: HashMap<String, String>,
        }

        impl ProvideEnv for CustomEnvProvider {
            fn get(&self, k: &str) -> Result<String, VarError> {
                self.vars.get(k).cloned().ok_or(VarError::NotPresent)
            }
        }

        let mut vars = HashMap::new();
        vars.insert("FOO".to_string(), "BAR".to_string());
        let env = Env::from_custom(CustomEnvProvider { vars });
        assert_eq!(env.get("FOO").unwrap(), "BAR");
        assert_eq!(
            env.get("OTHER").expect_err("not present"),
            VarError::NotPresent
        );
    }

    #[tokio::test]
    async fn custom_fs_round_trip() {
        #[derive(Debug)]
        struct InMemoryFs {
            files: Mutex<HashMap<PathBuf, Vec<u8>>>,
        }

        impl ProvideFs for InMemoryFs {
            fn read_to_end(
                &self,
                path: &Path,
            ) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<u8>>> + Send + '_>> {
                let path = path.to_path_buf();
                Box::pin(async move {
                    self.files
                        .lock()
                        .unwrap()
                        .get(&path)
                        .cloned()
                        .ok_or_else(|| std::io::ErrorKind::NotFound.into())
                })
            }

            fn write(
                &self,
                path: &Path,
                contents: &[u8],
            ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>> {
                let path = path.to_path_buf();
                let contents = contents.to_vec();
                Box::pin(async move {
                    self.files.lock().unwrap().insert(path, contents);
                    Ok(())
                })
            }
        }

        let provider = InMemoryFs {
            files: Mutex::new(HashMap::new()),
        };
        let fs = Fs::from_custom(provider);

        fs.read_to_end("/missing")
            .await
            .expect_err("file doesn't exist yet");

        fs.write("/test-file", b"hello")
            .await
            .expect("write succeeds");

        let result = fs.read_to_end("/test-file").await.expect("read succeeds");
        assert_eq!(result, b"hello");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn real_fs_write_sets_owner_only_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("secret.txt");
        let fs = Fs::real();

        fs.write(&path, b"sensitive").await.expect("write succeeds");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777; // mask off file type bits, keep only permission bits
        assert_eq!(mode, 0o600, "file should be owner read/write only");
    }

    #[tokio::test]
    async fn process_real_works() {
        let process = Process::real();
        let output = process.execute("echo hello").await.expect("success");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    #[tokio::test]
    async fn process_real_failure() {
        let process = Process::real();
        let output = process.execute("exit 42").await.expect("io succeeds");
        assert!(!output.status.success());
        assert_eq!(output.status.code(), Some(42));
    }

    #[test]
    fn exit_status_success() {
        assert!(ExitStatus::new(Some(0)).success());
        assert!(!ExitStatus::new(Some(1)).success());
        assert!(!ExitStatus::new(None).success());
        assert_eq!(ExitStatus::new(Some(0)).code(), Some(0));
        assert_eq!(ExitStatus::new(None).code(), None);
    }

    #[tokio::test]
    async fn process_from_slice_success() {
        let process = Process::from_slice(&[(
            "my-command",
            CommandOutput {
                status: ExitStatus::new(Some(0)),
                stdout: b"output of: my-command".to_vec(),
                stderr: Vec::new(),
            },
        )]);
        let output = process.execute("my-command").await.expect("success");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"output of: my-command");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn process_from_slice_failure() {
        let process = Process::from_slice(&[(
            "bad-command",
            CommandOutput {
                status: ExitStatus::new(Some(1)),
                stdout: Vec::new(),
                stderr: b"something went wrong".to_vec(),
            },
        )]);
        let output = process.execute("bad-command").await.expect("io succeeds");
        assert!(!output.status.success());
        assert_eq!(output.stderr, b"something went wrong");
    }

    #[tokio::test]
    async fn process_from_slice_not_found() {
        let process = Process::from_slice(&[]);
        let err = process.execute("missing").await.expect_err("should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn custom_process_works() {
        #[derive(Debug)]
        struct EchoProcess;

        impl ProvideProcess for EchoProcess {
            fn execute(
                &self,
                command: &str,
            ) -> Pin<Box<dyn Future<Output = std::io::Result<CommandOutput>> + Send + '_>>
            {
                let stdout = command.as_bytes().to_vec();
                Box::pin(async move {
                    Ok(CommandOutput {
                        status: ExitStatus::new(Some(0)),
                        stdout,
                        stderr: Vec::new(),
                    })
                })
            }
        }

        let process = Process::from_custom(EchoProcess);
        let output = process.execute("hello world").await.expect("success");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello world");
    }
}
