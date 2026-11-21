// JMESPath forms and functions over strings, enums, booleans, numbers, timestamps, blobs,
// documents, lists, maps, structures, and unions, each with a result type that suits its
// comparator or message. Neither implementation reports a problem.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [Describe]
}

@waitable(
    Fields: {
        acceptors: [
            { state: "success", matcher: { output: { path: "name", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "status", expected: "ready", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "ready", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "data", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "choice.text", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "doc.anything", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "@.name", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "`\"literal\"`", expected: "literal", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "'raw'", expected: "raw", comparator: "stringEquals" } } }
        ]
    }
    Comparisons: {
        acceptors: [
            { state: "success", matcher: { output: { path: "count > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "size >= ratio", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "name != status", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "ready && count > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "!ready", expected: "false", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "name || status", expected: "a", comparator: "stringEquals" } } }
        ]
    }
    Projections: {
        acceptors: [
            { state: "success", matcher: { output: { path: "items[].status", expected: "ready", comparator: "allStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[*].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[?status == 'ready'].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[?ready].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[?count > `1` && ready].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "ids[?@ == 'a']", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "groups[][].status", expected: "ready", comparator: "allStringEquals" } } }
            { state: "success", matcher: { output: { path: "groups[*][0].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[*].[name, status][]", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "labels.*", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "[name, status]", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "{n: name, s: status}.n", expected: "a", comparator: "stringEquals" } } }
        ]
    }
    Indexes: {
        acceptors: [
            { state: "success", matcher: { output: { path: "items[0].status", expected: "ready", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "items[-1].status", expected: "ready", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "items[:2].status", expected: "ready", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items[::-1].name", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "items | [0].status", expected: "ready", comparator: "stringEquals" } } }
        ]
    }
    Functions: {
        acceptors: [
            { state: "success", matcher: { output: { path: "length(items) > `2`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "length(name) == `3`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "length(groups[]) > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "contains(ids, 'a')", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "contains(keys(labels), 'env')", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "values(labels)", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "starts_with(name, 'a')", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "abs(count) > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "ceil(ratio) == floor(ratio)", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "sum(items[].count) > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "avg(items[].count) > `0`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "max(items[].count) > min(items[].count)", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "to_number('1') == `1`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "to_string(created)", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "join(', ', ids)", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "type(name)", expected: "string", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "not_null(name, status)", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "reverse(ids)", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "sort(ids)", expected: "a", comparator: "allStringEquals" } } }
            { state: "success", matcher: { output: { path: "to_array(name)", expected: "a", comparator: "anyStringEquals" } } }
            { state: "success", matcher: { output: { path: "map(&name, items)", expected: "a", comparator: "anyStringEquals" } } }
        ]
    }
    InputOutput: {
        acceptors: [
            { state: "success", matcher: { inputOutput: { path: "input.name", expected: "a", comparator: "stringEquals" } } }
            { state: "success", matcher: { inputOutput: { path: "input.name == output.name", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { inputOutput: { path: "contains(input.ids, output.name)", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { inputOutput: { path: "length(output.items) == length(input.ids)", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
    Messages: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { errorType: "NotFound" }, message: "name" }
            { state: "failure", matcher: { errorType: "NotFound" }, message: "status" }
            { state: "failure", matcher: { errorType: "NotFound" }, message: "ids" }
            { state: "failure", matcher: { errorType: "NotFound" }, message: "join(', ', ids)" }
            { state: "failure", matcher: { errorType: "NotFound" }, message: "to_string(count)" }
        ]
    }
)
operation Describe {
    input := {
        name: String
        ids: Ids
    }
    output := {
        name: String
        status: Status
        ready: Boolean
        count: Integer
        size: Long
        ratio: Double
        created: Timestamp
        data: Blob
        doc: Document
        labels: Labels
        ids: Ids
        items: Items
        groups: Groups
        choice: Choice
    }
    errors: [NotFound]
}

enum Status {
    READY = "ready"
    FAILED = "failed"
}

map Labels {
    key: String
    value: String
}

list Ids {
    member: String
}

list Items {
    member: Item
}

list Groups {
    member: Items
}

structure Item {
    name: String
    status: Status
    ready: Boolean
    count: Integer
}

union Choice {
    text: String
    number: Integer
}

@error("client")
structure NotFound {}
