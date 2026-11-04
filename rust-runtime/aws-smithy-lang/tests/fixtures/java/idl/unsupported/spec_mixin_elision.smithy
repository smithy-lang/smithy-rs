$version: "2"
namespace smithy.example

@mixin
structure IdBearer {
    id: String
}

structure IdRequired with [IdBearer] {
    @required
    $id
}
