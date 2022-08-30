$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

/// A service to test aspects of code generation where shapes have constraint traits.
@restJson1
@title("ConstraintsService")
service ConstraintsService {
    operations: [
        ConstrainedShapesOperation,
//      ConstrainedHttpBoundShapesOperation,
//      ConstrainedRecursiveShapesOperation,
//      // `httpQueryParams` and `httpPrefixHeaders` are structurually
//      // exclusive, so we need one operation per target shape type
//      // combination.
//      QueryParamsTargetingLengthMapOperation,
//      QueryParamsTargetingMapOfLengthStringOperation,
//      QueryParamsTargetingMapOfEnumStringOperation,
//      QueryParamsTargetingMapOfListOfLengthStringOperation,
//      QueryParamsTargetingMapOfSetOfLengthStringOperation,
//      QueryParamsTargetingMapOfListOfEnumStringOperation,
//      HttpPrefixHeadersTargetingLengthMapOperation,
//      // TODO(https://github.com/awslabs/smithy-rs/issues/1431)
//      // HttpPrefixHeadersTargetingMapOfEnumStringOperation,
    ],
}

@http(uri: "/constrained-shapes-operation", method: "GET")
operation ConstrainedShapesOperation {
    input: ConstrainedShapesOperationInputOutput,
    output: ConstrainedShapesOperationInputOutput,
    errors: [ErrorWithLengthStringMessage]
}

@http(uri: "/constrained-http-bound-shapes-operation/{lengthStringLabel}/{enumStringLabel}", method: "GET")
operation ConstrainedHttpBoundShapesOperation {
    input: ConstrainedHttpBoundShapesOperationInputOutput,
    output: ConstrainedHttpBoundShapesOperationInputOutput,
}

@http(uri: "/constrained-recursive-shapes-operation", method: "GET")
operation ConstrainedRecursiveShapesOperation {
    input: ConstrainedRecursiveShapesOperationInputOutput,
    output: ConstrainedRecursiveShapesOperationInputOutput,
}

@http(uri: "/query-params-targeting-length-map", method: "GET")
operation QueryParamsTargetingLengthMapOperation {
    input: QueryParamsTargetingLengthMapOperationInputOutput,
    output: QueryParamsTargetingLengthMapOperationInputOutput,
}

@http(uri: "/query-params-targeting-map-of-length-string-operation", method: "GET")
operation QueryParamsTargetingMapOfLengthStringOperation {
    input: QueryParamsTargetingMapOfLengthStringOperationInputOutput,
    output: QueryParamsTargetingMapOfLengthStringOperationInputOutput,
}

@http(uri: "/query-params-targeting-map-of-enum-string-operation", method: "GET")
operation QueryParamsTargetingMapOfEnumStringOperation {
    input: QueryParamsTargetingMapOfEnumStringOperationInputOutput,
    output: QueryParamsTargetingMapOfEnumStringOperationInputOutput,
}

@http(uri: "/query-params-targeting-map-of-list-of-length-string-operation", method: "GET")
operation QueryParamsTargetingMapOfListOfLengthStringOperation {
    input: QueryParamsTargetingMapOfListOfLengthStringOperationInputOutput,
    output: QueryParamsTargetingMapOfListOfLengthStringOperationInputOutput,
}

@http(uri: "/query-params-targeting-map-of-set-of-length-string-operation", method: "GET")
operation QueryParamsTargetingMapOfSetOfLengthStringOperation {
    input: QueryParamsTargetingMapOfSetOfLengthStringOperationInputOutput,
    output: QueryParamsTargetingMapOfSetOfLengthStringOperationInputOutput,
}

@http(uri: "/query-params-targeting-map-of-list-of-enum-string-operation", method: "GET")
operation QueryParamsTargetingMapOfListOfEnumStringOperation {
    input: QueryParamsTargetingMapOfListOfEnumStringOperationInputOutput,
    output: QueryParamsTargetingMapOfListOfEnumStringOperationInputOutput,
}

@http(uri: "/http-prefix-headers-targeting-length-map-operation", method: "GET")
operation HttpPrefixHeadersTargetingLengthMapOperation {
    input: HttpPrefixHeadersTargetingLengthMapOperationInputOutput,
    output: HttpPrefixHeadersTargetingLengthMapOperationInputOutput,
}

@http(uri: "/http-prefix-headers-targeting-map-of-enum-string-operation", method: "GET")
operation HttpPrefixHeadersTargetingMapOfEnumStringOperation {
    input: HttpPrefixHeadersTargetingMapOfEnumStringOperationInputOutput,
    output: HttpPrefixHeadersTargetingMapOfEnumStringOperationInputOutput,
}

structure ConstrainedShapesOperationInputOutput {
    @required
    conA: ConA,
}

structure ConstrainedHttpBoundShapesOperationInputOutput {
    @required
    @httpLabel
    lengthStringLabel: LengthString,

    @required
    @httpLabel
    enumStringLabel: EnumString,

    // TODO(https://github.com/awslabs/smithy-rs/issues/1394) `@required` not working
    // @required
    @httpPrefixHeaders("X-Prefix-Headers-")
    lengthStringHeaderMap: MapOfLengthString,

    @httpHeader("X-Length")
    lengthStringHeader: LengthString,

    // @httpHeader("X-Length-MediaType")
    // lengthStringHeaderWithMediaType: MediaTypeLengthString,

    @httpHeader("X-Length-Set")
    lengthStringSetHeader: SetOfLengthString,

    @httpHeader("X-Length-List")
    lengthStringListHeader: ListOfLengthString,

    // TODO(https://github.com/awslabs/smithy-rs/issues/1431)
    // @httpHeader("X-Enum")
    //enumStringHeader: EnumString,

    // @httpHeader("X-Enum-List")
    // enumStringListHeader: ListOfEnumString,

    @httpQuery("lengthString")
    lengthStringQuery: LengthString,

    @httpQuery("enumString")
    enumStringQuery: EnumString,

    @httpQuery("lengthStringList")
    lengthStringListQuery: ListOfLengthString,

    @httpQuery("lengthStringSet")
    lengthStringSetQuery: SetOfLengthString,

    @httpQuery("enumStringList")
    enumStringListQuery: ListOfEnumString,
}

structure HttpPrefixHeadersTargetingLengthMapOperationInputOutput {
    @httpPrefixHeaders("X-Prefix-Headers-LengthMap-")
    lengthMap: ConBMap,
}

structure HttpPrefixHeadersTargetingMapOfEnumStringOperationInputOutput {
    @httpPrefixHeaders("X-Prefix-Headers-MapOfEnumString-")
    mapOfEnumString: MapOfEnumString,
}

structure QueryParamsTargetingLengthMapOperationInputOutput {
    @httpQueryParams
    lengthMap: ConBMap
}

structure QueryParamsTargetingMapOfLengthStringOperationInputOutput {
    @httpQueryParams
    mapOfLengthString: MapOfLengthString
}

structure QueryParamsTargetingMapOfEnumStringOperationInputOutput {
    @httpQueryParams
    mapOfEnumString: MapOfEnumString
}

structure QueryParamsTargetingMapOfListOfLengthStringOperationInputOutput {
    @httpQueryParams
    mapOfListOfLengthString: MapOfListOfLengthString
}

structure QueryParamsTargetingMapOfSetOfLengthStringOperationInputOutput {
    @httpQueryParams
    mapOfSetOfLengthString: MapOfSetOfLengthString
}

structure QueryParamsTargetingMapOfListOfEnumStringOperationInputOutput {
    @httpQueryParams
    mapOfListOfEnumString: MapOfListOfEnumString
}

structure ConA {
    @required
    conB: ConB,

    optConB: ConB,

    lengthString: LengthString,
    minLengthString: MinLengthString,
    maxLengthString: MaxLengthString,
    fixedLengthString: FixedLengthString,

    conBList: ConBList,
    conBList2: ConBList2,

    conBSet: ConBSet,

    conBMap: ConBMap,

    mapOfMapOfListOfListOfConB: MapOfMapOfListOfListOfConB,

    constrainedUnion: ConstrainedUnion,
//  enumString: EnumString,
    // TODO Uncomment enumString from union when above line is uncommented.

//  listOfLengthString: ListOfLengthString,

//  setOfLengthString: SetOfLengthString,
}

map MapOfLengthString {
    key: LengthString,
    value: LengthString,
}

map MapOfEnumString {
    key: EnumString,
    value: EnumString,
}

map MapOfListOfLengthString {
    key: LengthString,
    value: ListOfLengthString,
}

map MapOfListOfEnumString {
    key: EnumString,
    value: ListOfEnumString,
}

map MapOfSetOfLengthString {
    key: LengthString,
    value: SetOfLengthString,
}

@length(min: 2, max: 8)
list LengthListOfLengthString {
    member: LengthString
}

@length(min: 2, max: 69)
string LengthString

@length(min: 2)
string MinLengthString

@length(min: 69)
string MaxLengthString

@length(min: 69, max: 69)
string FixedLengthString

@mediaType("video/quicktime")
@length(min: 1, max: 69)
string MediaTypeLengthString

/// A union with constrained members.
union ConstrainedUnion {
    //enumString: EnumString,
    lengthString: LengthString,

    constrainedStructure: ConB,
    conBList: ConBList,
    conBSet: ConBSet,
    conBMap: ConBMap,
}

@enum([
    {
        value: "t2.nano",
        name: "T2_NANO",
    },
    {
        value: "t2.micro",
        name: "T2_MICRO",
    },
    {
        value: "m256.mega",
        name: "M256_MEGA",
    }
])
string EnumString

set SetOfLengthString {
    member: LengthString
}

list ListOfLengthString {
    member: LengthString
}

list ListOfEnumString {
    member: EnumString
}

structure ConB {
    @required
    nice: String,
    @required
    int: Integer,

    optNice: String,
    optInt: Integer
}

structure ConstrainedRecursiveShapesOperationInputOutput {
    nested: RecursiveShapesInputOutputNested1,

    @required
    recursiveList: RecursiveList
}

structure RecursiveShapesInputOutputNested1 {
    @required
    recursiveMember: RecursiveShapesInputOutputNested2
}

structure RecursiveShapesInputOutputNested2 {
    @required
    recursiveMember: RecursiveShapesInputOutputNested1,
}

list RecursiveList {
    member: RecursiveShapesInputOutputNested1
}

list ConBList {
    member: NestedList
}

list ConBList2 {
    member: ConB
}

list NestedList {
    member: ConB
}

set ConBSet {
    member: NestedSet
}

set NestedSet {
    member: String
}

@length(min: 1, max: 69)
map ConBMap {
    key: String,
    value: LengthString
}

@error("client")
structure ErrorWithLengthStringMessage {
    // TODO Doesn't work yet because constrained string types don't implement
    // `AsRef<str>`.
    // @required
    // message: LengthString
}

map MapOfMapOfListOfListOfConB {
    key: String,
    value: MapOfListOfListOfConB
}

map MapOfListOfListOfConB {
    key: String,
    value: ConBList
}
