// Pagination cases that the reference implementation accepts with full validation:
//
// - `Service` has defaults for both tokens and the page size.
// - `ListAll` sets every property itself, with nested output paths.
// - `ListInherited` and `ListMixed` inherit different properties.
// - `ListShared` paginates differently in `Service` and `Other`.
// - `ListNested` is reached only through two resources.
// - `Unmarked` has no `@paginated` trait, although the service's defaults would resolve.
$version: "2"

namespace ex

@paginated(inputToken: "nextToken", outputToken: "nextToken", pageSize: "maxResults")
service Service {
    operations: [ListAll, ListInherited, ListMixed, ListShared, Unmarked]
    resources: [Outer]
}

@paginated(inputToken: "cursor", outputToken: "page.cursor")
service Other {
    operations: [ListShared]
}

service Bare {
    operations: [ListAll]
}

@readonly
@paginated(inputToken: "token", outputToken: "result.next", items: "result.items", pageSize: "size")
operation ListAll {
    input := { token: String, size: Integer, nextToken: String, maxResults: Integer }
    output := { result: Result, nextToken: String }
}

@readonly
@paginated(items: "things")
operation ListInherited {
    input := { nextToken: String, maxResults: Integer }
    output := { nextToken: String, things: Things }
}

@readonly
@paginated(inputToken: "start", pageSize: "limit")
operation ListMixed {
    input := { start: String, limit: Integer }
    output := { nextToken: String }
}

@readonly
@paginated(items: "things")
operation ListShared {
    input := { nextToken: String, maxResults: Integer, cursor: String }
    output := { nextToken: String, things: Things, page: Page }
}

@readonly
operation Unmarked {
    input := { nextToken: String }
    output := { nextToken: String }
}

resource Outer {
    resources: [Inner]
}

resource Inner {
    identifiers: { innerId: String }
    list: ListNested
}

@readonly
@paginated(items: "things")
operation ListNested {
    input := { nextToken: String, maxResults: Integer }
    output := { nextToken: String, things: Things }
}

structure Result {
    next: String
    items: Things
}

structure Page {
    cursor: String
}

list Things {
    member: String
}
