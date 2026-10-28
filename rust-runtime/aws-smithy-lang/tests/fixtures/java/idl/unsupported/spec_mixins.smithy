$version: "2"
namespace smithy.example

@mixin
structure BaseUser {
    userId: String
}

structure UserDetails with [BaseUser] {
    username: String
}

@mixin
@sensitive
string SensitiveString

@pattern("^[a-zA-Z\\.]*$")
string SensitiveText with [SensitiveString]
