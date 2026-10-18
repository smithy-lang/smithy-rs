$version: "2"

namespace smithy.example

// Define an example structure trait.
@trait
structure foo {}

@foo
string MyString1

@foo()
string MyString2

@foo({})
string MyString3

@tags
string MyString4

@tags()
string MyString5

@tags([])
string MyString6
