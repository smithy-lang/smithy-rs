# How to build

## Install smithy-cli

Refer to [Smithy installation](https://smithy.io/2.0/guides/smithy-cli/cli_installation.html) instructions

## Publish to Maven local

```bash
cd ~/smithy-rs/
./gradlew publishToMavenLocal
```

## Build sdk

```bash
cd ~/smithy-rs/h2-sample/sdk
./build.sh
```

## Run server

```bash
cd ~/smithy-rs/h2-sample/service
cargo r
```

## Curl request

```bash
curl -vv --http2-prior-knowledge localhost:8000/sample -X POST -H 'content-type: application/json' -d '{"inputValue": "some value"}'

```
