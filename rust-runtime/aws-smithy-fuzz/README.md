# aws-smithy-fuzz

AWS Smithy fuzz contains a set of utilities for writing fuzz tests against smithy-rs servers. This is part of our tooling to perform differential fuzzing against different versions of smithy-rs servers.

## Installation
1. Install `cargo afl`: `cargo install cargo-afl`
2. Install the AFL runtime: `cargo afl config --build`
2. Install the smithy CLI:
2. Install `aws-smithy-fuzz`:
    - Locally: `cargo afl install --path .`
    - From crates.io: cargo afl install aws-smithy-fuzz
   > **IMPORTANT**: This package MUST be installed with `cargo afl install` (instead of `cargo install`). If you do not use `afl`,
   > you will get linking errors.

## Usage
This contains a library + a CLI tool to fuzz smithy-rs servers. The library allows setting up a given smithy-rs server implementation as a `cdylib`. This allows two different versions two by dynamically linked at runtime and executed by the fuzzer.

Each of these components are meant to be usable independently:
1. The public APIs of `aws-smithy-fuzz` can be used to write your own fuzz targets without code generation.
2. The `lexicon.json` can be used outside of this project to seed a fuzzer from a Smithy model.
3. The fuzz driver can be used on other fuzz targets.

### Setup
First, you'll need to generate the 1 (or more) versions of a smithy-rs server to test against. The best way to do this is by using the smithy CLI. **This process is fully automated with the `aws-smithy-fuzz setup-smithy`. The following docs are in place in case you want to alter the behavior.**

There is nothing magic about what `setup-smithy` does, but it does save you some tedious setup.

```bash
aws-smithy-fuzz setup-smithy --revision fix-timestamp-from-f64 --service smithy.protocoltests.rpcv2Cbor#RpcV2Protocol --workdir fuzz-workspace-cbor2 --fuzz-runner-local-path smithy-rs --dependency software.amazon.smithy:smithy-protocol-tests:1.50.0 --rebuild-local-targets
```
<details>
<summary>
Details of functionality of `setup-smithy`. This can be helpful if you need to do something slightly different.
</summary>

```bash
# Create a workspace just to keep track of everything
mkdir workspace && cd workspace
REVISION_1=main
REVISION_2=76d5afb42d545ca2f5cbe90a089681135da935d3
rm -rf maven-locals && mkdir maven-locals
# Build two different versions of smithy-rs and publish them to two separate local directories
git clone https://github.com/smithy-lang/smithy-rs.git smithy-rs1 && (cd smithy-rs1 && git checkout $REVISION_1 && ./gradlew publishToMavenLocal -Dmaven.repo.local=$(cd ../maven-locals && pwd)/$REVISION_1)
git clone https://github.com/smithy-lang/smithy-rs.git smithy-rs2 && (cd smithy-rs2 && git checkout $REVISION_2 && ./gradlew publishToMavenLocal -Dmaven.repo.local=$(cd ../maven-locals && pwd)/$REVISION_2)
```

For each of these, use the smithy CLI to generate a server implementation using something like this:
```
{
    "version": "1.0",
    "maven": {
        "dependencies": [
            "software.amazon.smithy.rust.codegen.server.smithy:codegen-server:0.1.0",
            "software.amazon.smithy:smithy-aws-protocol-tests:1.50.0"
        ],
        "repositories": [
            {
                "url": "file://maven-locals/<INSERT REVISION>"
            },
            {
                "url": "https://repo1.maven.org/maven2"
            }
        ]
    },
    "projections": {
        "server": {
            "imports": [
            ],
            "plugins": {
                "rust-server-codegen": {
                    "runtimeConfig": {
                        "relativePath": "/Users/rcoh/code/smithy-rs/rust-runtime"
                    },
                    "codegen": {},
                    // PICK YOUR SERVICE
                    "service": "aws.protocoltests.restjson#RestJson",
                    "module": "rest_json",
                    "moduleVersion": "0.0.1",
                    "moduleDescription": "test",
                    "moduleAuthors": [
                        "protocoltest@example.com"
                    ]
                }
            }
        }
    }
}
```

Next, you'll use the `fuzzgen` target to generate two things based on your target crates:
1. A `lexicon.json` file: This uses information from the smithy model to seed the fuzzer with some initial inputs and helps it get better code coverage.
2. Fuzz target shims for your generated servers. These each implement most of the operations available in the smithy model and wire up each target crate with the correct bindings to create a cdylib crate that can be used by the fuzzer.

The easiest way to use `fuzzgen` is with the Smithy CLI:

```json
{
  "version": "1.0",
  "maven": {
    "dependencies": [
      "software.amazon.smithy.rust.codegen.serde:fuzzgen:0.1.0",
      "software.amazon.smithy:smithy-aws-protocol-tests:1.50.0"
    ]
  },
  "projections": {
    "harness": {
      "imports": [],
      "plugins": {
        "fuzz-harness": {
          "service": "aws.protocoltests.restjson#RestJson",
          "runtimeConfig": {
            "relativePath": "/Users/rcoh/code/smithy-rs/rust-runtime"
          },
          "targetCrates": [
            {
              "relativePath": "target-mainline/build/smithy/server/rust-server-codegen/",
              "name": "mainline"
            },
            {
              "relativePath": "target-previous-release/build/smithy/server/rust-server-codegen/",
              "name": "previous-release"
            }
          ]
        }
      }
    }
  }
}
```
</details>

### Initialization and Fuzzing
After `setup-smithy` creates the target shims, use `aws-smithy initialize` to setup ceremony required for `AFL` to function:
```
aws-smithy-fuzz initialize --lexicon <path to lexicon> --target-crate <path-to-target-b> --target-crate <path-to-target-a>
```

> **Important**: These are the crates generated by `fuzzgen`, not the crates you generated for the different smithy versions.

This may take a couple of minutes as it builds each crate.

To start the fuzz test use:
```
aws-smithy-fuzz fuzz
```

The fuzz session should start (although AFL may prompt you to run some configuration commands.)

You should see something like this:
```
   AFL ++4.21c {default} (/Users/rcoh/.cargo/bin/aws-smithy-fuzz) [explore]
┌─ process timing ────────────────────────────────────┬─ overall results ────┐
│        run time : 0 days, 0 hrs, 36 min, 18 sec     │  cycles done : 78    │
│   last new find : 0 days, 0 hrs, 0 min, 17 sec      │ corpus count : 1714  │
│last saved crash : 0 days, 0 hrs, 19 min, 25 sec     │saved crashes : 3     │
│ last saved hang : none seen yet                     │  saved hangs : 0     │
├─ cycle progress ─────────────────────┬─ map coverage┴──────────────────────┤
│  now processing : 145.173 (8.5%)     │    map density : 0.07% / 29.11%     │
│  runs timed out : 0 (0.00%)          │ count coverage : 1.52 bits/tuple    │
├─ stage progress ─────────────────────┼─ findings in depth ─────────────────┤
│  now trying : splice 12              │ favored items : 615 (35.88%)        │
│ stage execs : 11/12 (91.67%)         │  new edges on : 802 (46.79%)        │
│ total execs : 38.3M                  │ total crashes : 6 (3 saved)         │
│  exec speed : 16.0k/sec              │  total tmouts : 39 (0 saved)        │
├─ fuzzing strategy yields ────────────┴─────────────┬─ item geometry ───────┤
│   bit flips : 4/47.6k, 1/47.5k, 2/47.5k            │    levels : 27        │
│  byte flips : 0/5945, 0/5927, 0/5891               │   pending : 2         │
│ arithmetics : 14/415k, 0/826k, 0/821k              │  pend fav : 0         │
│  known ints : 0/53.4k, 3/224k, 0/329k              │ own finds : 1572      │
│  dictionary : 4/4.38M, 0/4.40M, 4/1.48M, 0/1.48M   │  imported : 0         │
│havoc/splice : 756/13.1M, 184/24.5M                 │ stability : 96.92%    │
│py/custom/rq : unused, unused, 581/381k, 1/183      ├───────────────────────┘
│    trim/eff : 32.14%/274k, 99.65%                  │             [cpu: 62%]
└─ strategy: explore ────────── state: in progress ──┘
```
(but with more pretty colors).

## Replaying Crashes

Run `aws-smithy-fuzz replay`. This will rerun all the crashes in the crashes folder. Other options exist, see: `aws-smithy-fuzz replay --help`.

**You can run replay from another terminal while fuzzing is in process.**

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
