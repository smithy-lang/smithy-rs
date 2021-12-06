/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// The SDK defaults to using RusTLS by default but you can also use [`native_tls`](https://github.com/sfackler/rust-native-tls)
/// which will choose a TLS implementation appropriate for your platform.
#[tokio::main]
async fn main() -> Result<(), aws_sdk_s3::Error> {
    tracing_subscriber::fmt::init();

    let shared_config = aws_config::load_from_env().await;

    let s3_config = aws_sdk_s3::Config::from(&shared_config);
    let client = aws_sdk_s3::Client::from_conf(s3_config);

    let resp = client.list_buckets().send().await?;

    for bucket in resp.buckets().unwrap_or_default() {
        println!("bucket: {:?}", bucket.name().unwrap_or_default())
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    /// You can run this test to ensure that this example is only using `native-tls`
    /// and that nothing is pulling in `rustls` as a dependency
    #[test]
    #[should_panic = "error: package ID specification `rustls` did not match any packages"]
    fn test_rustls_is_not_in_dependency_tree() {
        let cargo_command = std::process::Command::new("/Users/zhessler/.cargo/bin/cargo")
            .arg("tree")
            .arg("--invert")
            .arg("rustls")
            .output()
            .expect("failed to run 'cargo tree'");

        let stderr = String::from_utf8_lossy(&cargo_command.stderr);

        panic!("{}", stderr);
    }
}
