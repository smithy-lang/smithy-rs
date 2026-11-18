$version: "2"
namespace ex

resource User {
    identifiers: { name: String }
}

structure UserSummary for User {
    $age
}
