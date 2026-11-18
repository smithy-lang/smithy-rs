$version: "2"
namespace smithy.example

service ModelRepository {
    version: "2020-07-13",
    resources: [Model],
    operations: [PingService]
}

resource Model {}

operation PingService {
    input: PingServiceInput,
    output: PingServiceOutput,
    errors: [UnavailableError, BadRequestError]
}

@input
structure PingServiceInput {}

@output
structure PingServiceOutput {}

@error("server")
structure UnavailableError {}

@error("client")
structure BadRequestError {}
