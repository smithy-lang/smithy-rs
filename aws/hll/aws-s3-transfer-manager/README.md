# AWS S3 Transfer Manager

A high performance Amazon S3 client.


## Development

**Run all tests**

```sh
cargo test --all-features
```

**Run individual test**

```sh
cargo test --lib download::worker::tests::test_distribute_work
```

### Examples

**Copy**

See all options:
```sh
cargo run --example cp -- -h
```

**Download a file from S3**

```sh
AWS_PROFILE=<profile-name> RUST_LOG=trace cargo run --example cp s3://<my-bucket>/<my-key> /local/path/<filename>
```

NOTE: To run in release mode add `--release/-r` to the command, see `cargo run -h`.
NOTE: `trace` may be too verbose, you can see just this library's logs with `RUST_LOG=aws_s3_transfer_manager=trace`
