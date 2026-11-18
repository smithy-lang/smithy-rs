$version: "2"
namespace smithy.example

operation GetUser {
    input: GetUserInput
    output: GetUserOutput
}

@input
structure GetUserInput {
    userId: String
}

@output
structure GetUserOutput {
    username: String
    userId: String
}
