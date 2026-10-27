$version: "2"
namespace ex

structure UsesLater {
    // `String` is declared in this namespace by b.smithy, so it shadows the prelude.
    a: String
    b: Later
    c: FromJson
}

@laterTrait
string Annotated
