$version: "2"
namespace smithy.example

resource SprocketResource {
    identifiers: {
        sprocketId: String,
    },
    read: GetSprocket,
}

@readonly
operation GetSprocket {
    input := {
        @required
        sprocketId: String
    }
}
