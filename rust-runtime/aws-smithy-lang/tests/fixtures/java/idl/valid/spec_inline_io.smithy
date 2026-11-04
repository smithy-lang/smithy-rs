$version: "2"
namespace smithy.example

operation GetUser {
    // The generated shape name is GetUserInput
    input := {
        userId: String
    }

    // The generated shape name is GetUserOutput
    output := {
        username: String
        userId: String
    }
}
