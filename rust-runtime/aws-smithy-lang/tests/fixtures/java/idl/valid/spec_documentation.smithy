$version: "2"
namespace smithy.example

/// This is documentation about a shape.
///
/// - This is a list
/// - More of the list.
string MyString

/// This is documentation about a trait shape.
///   More docs here.
@trait
structure myTrait {}

/// A deprecated string.
@deprecated
string DeprecatedString

/// Documentation about the structure.
structure Example {
    /// Documentation about the member.
    @required
    foo: String,
}
