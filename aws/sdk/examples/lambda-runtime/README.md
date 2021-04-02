# Using the AWS SDK for Rust with the [Rust Runtime for AWS Lambda](https://github.com/awslabs/aws-lambda-rust-runtime)

## How-To guide for building this example

Install the Rust compiler. I use the nightly compiler because it lets me use the llvm source coverage flags, stable should work too if you don't have a sense of wonder and adventure.

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly
source $HOME/.cargo/env
```

Create a new project

```bash
cargo new my-lambda-function
```

Until the Rust SDK is public, we need to manually fetch it and build it.

<details>
  <summary>SSH Keys</summary>
  
  If the AWS SDK for Rust repository is still private, you need to make sure your SSH Keys are set up for github. 
  ```bash
  eval "$(ssh-agent -s)"
  ssh-add ~/.ssh/github 
  ```
</details>

We are explicitly pulling the SDK into the Lambda Function folder because of some odd behavior in the way it gets packaged by SAM local.
```bash
git clone git@github.com:awslabs/smithy-rs.git ./my-lambda-function/vendored
cd ./my-lambda-function/vendored
./gradlew :aws:sdk:assemble
# This isn't a typo. The Rust SDK motto is "An SDK so nice, you have to build it twice!!"
./gradlew :aws:sdk:assemble
cd ..
```

Add dependencies on Rust SDK and binary name/path to `Cargo.toml`

```lang-yaml
[dependencies]

## Delete this line and uncomment the following when the first release after v0.5-alpha drops
dynamodb = { path = "./vendored/aws/sdk/build/aws-sdk/dynamodb", features = ["client"]}
# dynamodb = { git = "ssh://git@github.com/awslabs/smithy-rs.git", rev = "v0.5-alpha.cargo", features = ["fluent"]}
aws-hyper = { git = "ssh://git@github.com/awslabs/smithy-rs.git", rev = "v0.5-alpha.cargo" }
tokio = { version = "1", features = ["full"] }

serde = "1.0.82"
serde_derive = "1.0.82"
serde_json = { version = "1.0.33", features = ["raw_value"] }
simple_logger = "1.6.0"


lambda_runtime = "0.3.0"

log = "^0.4"

[[bin]]
name = "bootstrap"
path = "src/main.rs"
```

Update `src/main.rs`

```rust
use lambda_runtime::{handler_fn, Context};
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use simple_logger::SimpleLogger;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;


#[derive(Deserialize)]
struct Request {
    command: String,
}

#[derive(Serialize)]
struct Response {
    req_id: String,
    msg: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // required to enable CloudWatch error logging by the runtime
    // can be replaced with any other method of initializing `log`
    SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();

    let func = handler_fn(my_handler);
    lambda_runtime::run(func).await?;
    Ok(())
}

pub(crate) async fn my_handler(event: Request, ctx: Context) -> Result<Response, Error> {
    let command = event.command;

    let client = dynamodb::Client::from_env();
    let tables = client.list_tables().send().await?;
    println!("Current DynamoDB tables: {:?}", tables);

    // prepare the response
    let resp = Response {
        req_id: ctx.request_id,
        msg: format!("Command {} executed.", command),
    };

    Ok(resp)
}
```

Next, we're going to define a Lambda Function in AWS CloudFormation. First, create the template file in teh root of the project directory (next to your `Cargo.toml` file)

```bash
cat << EOF > ./template.yaml
Transform: AWS::Serverless-2016-10-31
Resources:
  HelloRustFunction:
    Type: AWS::Serverless::Function
    Properties:
      FunctionName: HelloRust
      Handler: bootstrap.is.real.handler
      Runtime: provided.al2
      MemorySize: 512
      CodeUri: .
      Policies:
      - AmazonDynamoDBFullAccess
    Metadata:
      BuildMethod: makefile
EOF
```

Now we're ready to build. First we need to revert to the last known version of the SAM CLI that worked with provided runtimes.

```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install aws-sam-cli==1.12.0
sam build
```

Now we're ready to test the Function locally. First we'll write a sample payload to send to the function.

```bash
cat << EOF > ./event1.json
{
    "command": "FOO"
}
EOF
```

Now we're ready to test. By default, when you run `sam build` it will place the build artifacts in a directory named `./.aws-sam/build/`. The following command will tell the SAM local CLI to invoke the Lambda Function defined by `HelloRustFunction` in the template located at `./.aws-sam/build/template.yaml`and to use the contents of the file `./event.json` as the input to the invocation

```bash
sam local invoke -t ./.aws-sam/build/template.yaml -e ./event.json HelloRustFunction
```

The console output shows that the `provided.al2` image was used. This is the OS used by the AWS Lambda service when Functions are run. We also see the the Function printed the name of all of the tables in my account/region (yours may differ).


<pre><code class="language-text">
(.venv) Feder08:~/environment/my-lambda-function (main) $ sam local invoke -t ./.aws-sam/build/template.yaml -e ./event.json HelloRustFunction
Invoking bootstrap.is.real.handler <mark>(provided.al2)</mark>
Image was not found.
Building image....................
Skip pulling image and use local one: amazon/aws-sam-cli-emulation-image-provided.al2:rapid-1.12.0.

Mounting /home/ec2-user/environment/my-lambda-function/.aws-sam/build/HelloRustFunction as /var/task:ro,delegated inside runtime container
START RequestId: 14cd73d5-530e-1e50-f524-c542373bb78a Version: $LATEST
2021-04-02 22:00:44,986 INFO  [smithy_http_tower::parse_response] send_operation
2021-04-02 22:00:44,987 INFO  [smithy_http_tower::parse_response] send_operation; operation="ListTables"
2021-04-02 22:00:44,987 INFO  [smithy_http_tower::parse_response] send_operation; service="dynamodb"
2021-04-02 22:00:45,014 INFO  [smithy_http_tower::parse_response] send_operation; status="ok"
Current DynamoDB tables: <mark>ListTablesOutput { table_names: Some(["ManualTable"]), last_evaluated_table_name: None }</mark>
END RequestId: 14cd73d5-530e-1e50-f524-c542373bb78a
REPORT RequestId: 14cd73d5-530e-1e50-f524-c542373bb78a  Init Duration: 231.71 ms        Duration: 54.37 ms      Billed Duration: 100 ms Memory Size: 512 MB     Max Memory Used: 16 MB

{"req_id":"14cd73d5-530e-1e50-f524-c542373bb78a","msg":"Command FOO executed."}
</code></pre>