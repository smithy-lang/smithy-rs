$version: "2"
$operationInputSuffix: "Request"
$operationOutputSuffix: "Response"

namespace smithy.example

operation GetUser {
    // The generated shape name is GetUserRequest
    input := {
        userId: String
    }

    // The generated shape name is GetUserResponse
    output := {
        username: String
        userId: String
    }
}
