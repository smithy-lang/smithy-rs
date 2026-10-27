// Leading comment
$version: "2" // trailing comment on a control statement
//
//no space after slashes
metadata m = [ // comment inside an array
    1, // comment after a value
    2
]

namespace ex

/// Documentation applies.
@deprecated
/// Documentation after a trait is an ordinary comment.
string A

/// A documentation comment still applies across a blank line.

string B

structure C {
    /// Member docs
    ///Second line without a space
    a: String // trailing comment
    , b: String
}
// Comment at end of file without newline
