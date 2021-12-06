# CI CDK

This is the CDK infrastructure as code for awslabs/smithy-rs and awslabs/aws-sdk-rust
continuous integration.

The `cdk.json` file tells the CDK Toolkit how to synthesize the infrastructure.

## Useful commands

-   `npm run lint`: lint code
-   `npm run format`: auto-format code
-   `npm run build`: compile typescript to js
-   `npm run watch`: watch for changes and compile
-   `npm run test`: perform the jest unit tests
-   `npx cdk deploy`: deploy this stack to your default AWS account/region
-   `npx cdk diff`: compare deployed stack with current state
-   `npx cdk synth`: emits the synthesized CloudFormation template
