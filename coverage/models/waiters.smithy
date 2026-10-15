// coverage-service: test.waiters#WaiterService
// coverage-plugins: client
$version: "2.0"

namespace test.waiters

use aws.protocols#awsJson1_0
use smithy.waiters#waitable

@awsJson1_0
service WaiterService {
    operations: [GetEntity]
}

@waitable(
    // field / subexpression traversal + booleanEquals
    BooleanFieldReady: {
        acceptors: [
            { state: "success", matcher: { output: { path: "primitives.boolean", expected: "true", comparator: "booleanEquals" } } }
            { state: "failure", matcher: { output: { path: "primitives.requiredBoolean", expected: "false", comparator: "booleanEquals" } } }
            { state: "retry", matcher: { errorType: "NotFoundException" } }
        ]
    }
    // string comparisons + enums
    StringFieldReady: {
        acceptors: [
            { state: "success", matcher: { output: { path: "primitives.string", expected: "done", comparator: "stringEquals" } } }
            { state: "success", matcher: { output: { path: "primitives.enum", expected: "two", comparator: "stringEquals" } } }
            { state: "failure", matcher: { output: { path: "primitives.requiredString", expected: "failed", comparator: "stringEquals" } } }
        ]
    }
    // flatten projections + allStringEquals/anyStringEquals
    ListProjections: {
        acceptors: [
            { state: "success", matcher: { output: { path: "lists.structs[].string", expected: "ok", comparator: "allStringEquals" } } }
            { state: "failure", matcher: { output: { path: "lists.structs[].primitives.string", expected: "bad", comparator: "anyStringEquals" } } }
            { state: "retry", matcher: { output: { path: "lists.enums[]", expected: "one", comparator: "anyStringEquals" } } }
        ]
    }
    // functions: length, contains; comparisons on numbers; literals
    FunctionMatchers: {
        acceptors: [
            { state: "success", matcher: { output: { path: "length(lists.structs[]) == `2`", expected: "true", comparator: "booleanEquals" } } }
            { state: "success", matcher: { output: { path: "contains(lists.strings, 'two')", expected: "true", comparator: "booleanEquals" } } }
            { state: "failure", matcher: { output: { path: "contains(lists.integers, primitives.integer)", expected: "true", comparator: "booleanEquals" } } }
            { state: "retry", matcher: { output: { path: "length(primitives.string) > `3`", expected: "true", comparator: "booleanEquals" } } }
            { state: "retry", matcher: { output: { path: "primitives.integer >= `4` && primitives.long < `9`", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
    // object projections, filter projections, multi-select lists, keys()
    ProjectionMatchers: {
        acceptors: [
            { state: "success", matcher: { output: { path: "maps.structs.*.string", expected: "ok", comparator: "allStringEquals" } } }
            { state: "success", matcher: { output: { path: "lists.structs[?integer > `1`].string", expected: "big", comparator: "anyStringEquals" } } }
            { state: "failure", matcher: { output: { path: "lists.structs[*].[string, primitives.string][]", expected: "oops", comparator: "anyStringEquals" } } }
            { state: "retry", matcher: { output: { path: "keys(maps.strings)", expected: "pending-key", comparator: "anyStringEquals" } } }
        ]
    }
    // inputOutput matchers exercise named bindings
    InputOutputMatchers: {
        acceptors: [
            { state: "success", matcher: { inputOutput: { path: "input.name == output.primitives.string", expected: "true", comparator: "booleanEquals" } } }
            { state: "failure", matcher: { inputOutput: { path: "input.trueBool && output.primitives.boolean", expected: "false", comparator: "booleanEquals" } } }
        ]
    }
    // boolean operations and not-expressions
    BooleanLogic: {
        acceptors: [
            { state: "success", matcher: { output: { path: "!(primitives.boolean) || primitives.requiredBoolean", expected: "true", comparator: "booleanEquals" } } }
            { state: "failure", matcher: { output: { path: "primitives.string != 'pending'", expected: "true", comparator: "booleanEquals" } } }
            { state: "retry", matcher: { success: false } }
        ]
    }
)
operation GetEntity {
    input: GetEntityRequest
    output: GetEntityResponse
    errors: [NotFoundException]
}

@error("client")
structure NotFoundException {
    message: String
}

structure GetEntityRequest {
    @required
    name: String
    trueBool: Boolean
    falseBool: Boolean
}

structure GetEntityResponse {
    primitives: EntityPrimitives
    lists: EntityLists
    maps: EntityMaps
}

structure EntityPrimitives {
    boolean: Boolean
    string: String
    byte: Byte
    short: Short
    integer: Integer
    long: Long
    float: Float
    double: Double
    enum: Enum
    intEnum: IntEnum
    @required
    requiredBoolean: Boolean
    @required
    requiredString: String
}

structure EntityLists {
    booleans: BooleanList
    strings: StringList
    shorts: ShortList
    integers: IntegerList
    longs: LongList
    floats: FloatList
    doubles: DoubleList
    enums: EnumList
    intEnums: IntEnumList
    structs: StructList
}

structure EntityMaps {
    booleans: BooleanMap
    strings: StringMap
    integers: IntegerMap
    enums: EnumMap
    intEnums: IntEnumMap
    structs: StructMap
}

enum Enum {
    ONE = "one"
    TWO = "two"
}

intEnum IntEnum {
    ONE = 1
    TWO = 2
}

structure Struct {
    @required
    requiredInteger: Integer
    primitives: EntityPrimitives
    strings: StringList
    integer: Integer
    string: String
    enums: EnumList
    subStructs: SubStructList
}

structure SubStruct {
    subStructPrimitives: EntityPrimitives
}

list BooleanList { member: Boolean }
list StringList { member: String }
list ShortList { member: Short }
list IntegerList { member: Integer }
list LongList { member: Long }
list FloatList { member: Float }
list DoubleList { member: Double }
list EnumList { member: Enum }
list IntEnumList { member: IntEnum }
list StructList { member: Struct }
list SubStructList { member: SubStruct }

map BooleanMap { key: String, value: Boolean }
map StringMap { key: String, value: String }
map IntegerMap { key: String, value: Integer }
map EnumMap { key: String, value: Enum }
map IntEnumMap { key: String, value: IntEnum }
map StructMap { key: String, value: Struct }
