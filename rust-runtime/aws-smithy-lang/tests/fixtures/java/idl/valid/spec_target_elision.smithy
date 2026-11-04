$version: "2"
namespace smithy.example

resource User {
    identifiers: {
        name: String
        uuid: String
    }
}

structure UserSummary for User {
    $name
    age: Short
}
