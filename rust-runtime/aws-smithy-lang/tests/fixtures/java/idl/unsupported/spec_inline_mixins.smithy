$version: "2"
namespace smithy.example

@mixin
structure BaseUser {
    userId: String
}

resource User {}

operation GetUser {
    input := @references([{resource: User}]) {
        userId: String
    }

    output := with [BaseUser] {
        username: String
    }
}

operation PutUser {
    input :=
        @references([{resource: User}])
        with [BaseUser] {}
}
