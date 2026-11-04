$version: "2"
namespace smithy.hello

use smithy.example#Foo
use smithy.example#Baz
use smithy.example#test
use smithy.example#anotherTrait

map MyMap {
    // Resolves to smithy.example#Foo
    key: Foo
    // Resolves to smithy.example#Baz
    value: Baz
}

@test // <-- Resolves to smithy.example#test
@anotherTrait
string MyString
