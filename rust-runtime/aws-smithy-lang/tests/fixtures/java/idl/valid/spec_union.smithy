$version: "2"
namespace smithy.example

union MyUnion {
    i32: Integer

    @length(min: 1, max: 100)
    string: String

    time: Timestamp
}
