$version: "2"
namespace smithy.example

use foo.baz#Bar

string MyString

structure MyStructure {
    // Resolves to smithy.example#MyString
    a: MyString

    // Resolves to smithy.example#MyString
    b: smithy.example#MyString

    // Resolves to foo.baz#Bar
    c: Bar

    // Resolves to smithy.api#String
    d: String

    // Resolves to smithy.example#MyBoolean (forward reference)
    e: MyBoolean

    // Resolves to smithy.example#Other, declared in another file.
    g: Other
}

boolean MyBoolean
