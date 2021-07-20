/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::environment;
use std::borrow::Cow;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// In-memory source of profile data
pub struct Source {
    /// Contents and path of ~/.aws/config
    pub config_file: File,

    /// Contents and path of ~/.aws/credentials
    pub credentials_file: File,

    /// Profile to use
    ///
    /// Overridden via `$AWS_PROFILE`, defaults to `default`
    pub profile: Cow<'static, str>,
}

/// In-memory configuration file
pub struct File {
    pub path: String,
    pub contents: String,
}

/// Load a [Source](Source) from a given environment and filesystem.
pub fn load(proc_env: &environment::ProcessEnvironment, fs: &environment::Fs) -> Source {
    let config = tracing::info_span!("load_config_file")
        .in_scope(|| read(&fs, &proc_env, "~/.aws/config", "AWS_CONFIG_FILE"));
    let credentials = tracing::info_span!("load_credentials_file").in_scope(|| {
        read(
            &fs,
            &proc_env,
            "~/.aws/credentials",
            "AWS_SHARED_CREDENTIALS_FILE",
        )
    });
    Source {
        config_file: config,
        credentials_file: credentials,
        profile: proc_env
            .get("AWS_PROFILE")
            .map(Cow::Owned)
            .unwrap_or(Cow::Borrowed("default")),
    }
}

/// Read a file given a potential path override & Home directory expansion
///
/// Arguments:
/// * `fs`: Filesystem abstraction
/// * `environment`: Process environment abstraction
/// * `default_path`: Fallback path if the environment variable specified by `overriden_by_env_var` is unset
/// * `overridden_by_env_var`: name of an environment variable whose contents can override `default_path`
fn read(
    fs: &environment::Fs,
    environment: &environment::ProcessEnvironment,
    default_path: &str,
    overridden_by_env_var: &str,
) -> File {
    let path = environment
        .get(overridden_by_env_var)
        .map(Cow::Owned)
        .ok()
        .unwrap_or_else(|| default_path.into());
    let expanded = expand_home(path.as_ref(), &environment, Os::real());
    tracing::debug!(before = ?path, after = ?expanded, "home directory expanded");
    let data = match fs.read(&expanded) {
        Ok(data) => data,
        Err(e) => {
            match e.kind() {
                ErrorKind::NotFound if path == default_path => {
                    tracing::info!(path = %path, "config file not found")
                }
                ErrorKind::NotFound if path != default_path => {
                    tracing::warn!(path = %path, env = %overridden_by_env_var, "config file overridden via environment variable not found")
                }
                _other => tracing::warn!(path = %path, error = %e, "failed to read config file"),
            };
            Default::default()
        }
    };
    let data = match String::from_utf8(data) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!(path = %path, error = %e, "config file did not contain utf-8 encoded data");
            Default::default()
        }
    };
    tracing::info!(path = %path, size = ?data.len(), "config file loaded");
    File {
        // lossy is OK here, the name of this file is just for debugging purposes
        path: expanded.to_string_lossy().into(),
        contents: data,
    }
}

fn expand_home(
    path: impl AsRef<Path>,
    env_var: &environment::ProcessEnvironment,
    os: Os,
) -> PathBuf {
    let path = path.as_ref();
    let mut components = path.components();
    let start = components.next();
    match start {
        None => path.into(), // empty path,
        Some(Component::Normal(s)) if s == "~" => {
            // do homedir replacement
            let mut path = match home_dir(&env_var, os) {
                Some(dir) => {
                    tracing::debug!(home = ?dir, "performing home directory substitution");
                    dir
                }
                None => {
                    tracing::warn!(
                        "could not determine home directory but home expansion was requested"
                    );
                    Default::default()
                }
            };
            // rewrite the path using system-specific path separators
            for component in components {
                path.push(component);
            }
            path
        }
        // Finally, handle the case where it doesn't begin with some version of `~/`:
        // NOTE: in this case we aren't performing path rewriting. This is correct because
        // this path comes from an environment variable on the target
        // platform, so in that case, the separators should already be correct.
        _other => path.into(),
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Os {
    Windows,
    NotWindows,
}

impl Os {
    pub fn real() -> Self {
        match std::env::consts::OS {
            "windows" => Os::Windows,
            _ => Os::NotWindows,
        }
    }
}

/// Resolve a home directory given a set of environment variables
fn home_dir(env_var: &environment::ProcessEnvironment, os: Os) -> Option<PathBuf> {
    if let Ok(home) = env_var.get("HOME") {
        tracing::debug!(src = "HOME", "loaded home directory");
        return Some(PathBuf::from(home));
    }

    if os == Os::Windows {
        if let Ok(home) = env_var.get("USERPROFILE") {
            tracing::debug!(src = "USERPROFILE", "loaded home directory");
            return Some(PathBuf::from(home));
        }

        let home_drive = env_var.get("HOMEDRIVE");
        let home_path = env_var.get("HOMEPATH");
        tracing::debug!(src = "HOMEDRIVE/HOMEPATH", "loaded home directory");
        if let (Ok(mut drive), Ok(path)) = (home_drive, home_path) {
            drive.push_str(&path);
            return Some(drive.into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::environment::{Fs, ProcessEnvironment};
    use crate::profile::source::{expand_home, load, Os};
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::error::Error;
    use std::fs;

    #[test]
    fn only_expand_home_prefix() {
        // ~ is only expanded as a single component (currently)
        let path = "~aws/config";
        let env = ProcessEnvironment::from_slice(&[("HOME", "/user/foo")]);
        assert_eq!(
            expand_home(&path, &env, Os::NotWindows).to_str().unwrap(),
            "~aws/config"
        );
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    struct SourceTests {
        tests: Vec<TestCase>,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    struct TestCase {
        name: String,
        environment: HashMap<String, String>,
        platform: String,
        profile: Option<String>,
        config_location: String,
        credentials_location: String,
    }

    /// Run all tests from file-location-tests.json
    #[test]
    fn run_tests() -> Result<(), Box<dyn Error>> {
        let tests = fs::read_to_string("test-data/file-location-tests.json")?;
        let tests: SourceTests = serde_json::from_str(&tests)?;
        for (i, test) in tests.tests.into_iter().enumerate() {
            eprintln!("test: {}", i);
            check(test);
        }
        Ok(())
    }

    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn logs_produced_default() {
        let env = ProcessEnvironment::from_slice(&[("HOME", "/user/name")]);
        let mut fs = HashMap::new();
        fs.insert(
            "/user/name/.aws/config".to_string(),
            "[default]\nregion = us-east-1".into(),
        );

        let fs = Fs::from_map(fs);

        let _src = load(&env, &fs);
        assert!(logs_contain("config file loaded"));
        assert!(logs_contain("performing home directory substitution"));
    }

    fn check(test_case: TestCase) {
        let fs = Fs::real();
        let env = ProcessEnvironment::from(test_case.environment);
        let platform_matches = (cfg!(windows) && test_case.platform == "windows")
            || (!cfg!(windows) && test_case.platform != "windows");
        if platform_matches {
            let source = load(&env, &fs);
            if let Some(expected_profile) = test_case.profile {
                assert_eq!(source.profile, expected_profile, "{}", &test_case.name);
            }
            assert_eq!(
                source.config_file.path, test_case.config_location,
                "{}",
                &test_case.name
            );
            assert_eq!(
                source.credentials_file.path, test_case.credentials_location,
                "{}",
                &test_case.name
            )
        } else {
            println!(
                "NOTE: ignoring test case for {} which does not apply to our platform: \n  {}",
                &test_case.platform, &test_case.name
            )
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_expand_home() {
        let path = "~/.aws/config";
        let env = ProcessEnvironment::from_slice(&[("HOME", "/user/foo")]);
        assert_eq!(
            expand_home(&path, &env, Os::NotWindows).to_str().unwrap(),
            "/user/foo/.aws/config"
        );
    }

    /// Test that a linux oriented path expands on windows
    #[test]
    #[cfg(windows)]
    fn test_expand_home_windows() {
        let path = "~/.aws/config";
        let env =
            ProcessEnvironment::from_slice(&[("HOMEDRIVE", "C:"), ("HOMEPATH", "\\Users\\name")]);
        assert_eq!(
            expand_home(&path, &env, Os::Windows).to_str().unwrap(),
            "C:\\Users\\name\\.aws\\config"
        );
    }

    /// Test that windows oriented path expands on windows
    #[test]
    #[cfg(windows)]
    fn test_expand_windows_path_windows() {
        let path = "~\\.aws\\config";
        let env =
            ProcessEnvironment::from_slice(&[("HOMEDRIVE", "C:"), ("HOMEPATH", "\\Users\\name")]);
        assert_eq!(
            expand_home(&path, &env, Os::Windows).to_str().unwrap(),
            "C:\\Users\\name\\.aws\\config"
        );
    }
}
