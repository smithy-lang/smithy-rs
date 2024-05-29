/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[derive(Debug)]
pub struct VersionerOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct TestBase {
    _tmp: TempDir,
    pub test_data: Utf8PathBuf,
    pub root: Utf8PathBuf,
}

impl TestBase {
    pub fn new(branch_name: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().join("test_base")).unwrap();

        let test_data =
            Utf8PathBuf::try_from(std::env::current_dir().unwrap().join("test_data")).unwrap();
        let tar_path = test_data.join("test_base.git.tar.gz");
        assert!(
            Command::new("tar")
                .args(["xfz", tar_path.as_str()])
                .current_dir(tmp.path())
                .status()
                .unwrap()
                .success(),
            "untarring the test_base into the temp directory failed"
        );
        // ensure unpacked file has the current user assigned to the owner,
        // otherwise we could get the `fatal: detected dubious ownership in repository` error
        let test_repo = Utf8PathBuf::try_from(tmp.path().join("test_base.git")).unwrap();
        assert!(
            Command::new("chown")
                .args(&[&format!("{}", whoami::username()), test_repo.as_str(),])
                .current_dir(tmp.path())
                .status()
                .unwrap()
                .success(),
            "setting the owner of `test_base.git` to the current user failed"
        );
        assert!(
            Command::new("git")
                .args(["clone", "test_base.git"])
                .current_dir(tmp.path())
                .status()
                .unwrap()
                .success(),
            "cloning the test_base repo failed"
        );
        assert!(root.exists(), "test_base not found after cloning");
        change_branch(&root, branch_name);

        Self {
            _tmp: tmp,
            test_data,
            root,
        }
    }

    pub fn change_branch(&self, branch_name: &str) {
        change_branch(&self.root, branch_name);
    }

    pub fn run_versioner(&self, args: &[&str], expect_failure: bool) -> VersionerOutput {
        let mut cmd = test_bin::get_test_bin("runtime-versioner");
        let cmd = cmd.args(args).current_dir(&self.root);
        let output = cmd.output().expect("failed to execute runtime-versioner");
        let status = output.status.code().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("###\ncmd: {cmd:?}\nstatus: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}\n\n");
        if expect_failure {
            assert!(
                !output.status.success(),
                "expected runtime-versioner to fail, but it succeeded"
            );
        } else {
            assert!(
                output.status.success(),
                "expected runtime-versioner to succeed, but it failed"
            );
        }
        VersionerOutput {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

fn change_branch(path: &Utf8Path, branch_name: &str) {
    assert!(
        Command::new("git")
            .args(["checkout", branch_name])
            .current_dir(path)
            .status()
            .unwrap()
            .success(),
        "changing to the correct test_base branch failed"
    );
}
