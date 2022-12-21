# CI CDK

This is the CDK infrastructure as code for awslabs/smithy-rs and awslabs/aws-sdk-rust
continuous integration.

The `cdk.json` file tells the CDK Toolkit how to synthesize the infrastructure.

## Canary local development

Sometimes it's useful to only deploy the canary resources to a test AWS account to iterate
on the `canary-runner` and `canary-lambda`. To do this, run the following:

```bash
npm install
npm run build
npx cdk --app "node build/bin/canary-only.js" synth
npx cdk --app "node build/bin/canary-only.js" deploy --outputs-file cdk-outputs.json
```

From there, you can just point the `canary-runner` to the `cdk-outputs.json` to run it:

```bash
cd canary-runner
cargo run -- run --sdk-release-tag <version> --musl --cdk-outputs ../cdk-outputs.json
```

__NOTE:__ You may want to add a `--profile` to the `deploy` command to select a specific credential
profile to deploy to if you don't want to use the default.

Also, if this is a new test AWS account, be sure it CDK bootstrap it before attempting to deploy.

## Useful commands

-   `npm run lint`: lint code
-   `npm run format`: auto-format code
-   `npm run build`: compile typescript to js
-   `npm run watch`: watch for changes and compile
-   `npm run test`: perform the jest unit tests
-   `npx cdk deploy`: deploy this stack to your default AWS account/region
-   `npx cdk diff`: compare deployed stack with current state
-   `npx cdk synth`: emits the synthesized CloudFormation template
