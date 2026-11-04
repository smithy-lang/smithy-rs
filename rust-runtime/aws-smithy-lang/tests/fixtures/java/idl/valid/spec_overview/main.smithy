// (1) Control section
$version: "2"

// (2) Metadata section
metadata foo = "bar"

// (3) Shape section
namespace smithy.example

use smithy.other.namespace#MyString

structure MyStructure {
    @required
    foo: MyString
}
