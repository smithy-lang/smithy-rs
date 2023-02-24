$version: "1.0"
namespace naming_obs_structs

use aws.protocols#awsJson1_1
use aws.api#service

/// Confounds model generation machinery with lots of problematic names
@awsJson1_1
@service(sdkId: "NamingObstacleCourseStructs")
service NamingObstacleCourseStructs {
    version: "2006-03-01",
    operations: [
       Structs,
    ]
}

structure SomethingElse {
    result: Result,
    resultList: ResultList,
    option: Option,
    optionList: OptionList,
    someUnion: SomeUnion,
}

union SomeUnion {
    Result: Result,
    Option: Option,
}

structure Result {
    value: String,
}
list ResultList {
    member: Result,
}
structure Option {
    value: String,
}
list OptionList {
    member: Result,
}

structure StructsInputOutput {
    result: Result,
    resultList: ResultList,
    option: Option,
    optionList: OptionList,
    somethingElse: SomethingElse,
}
operation Structs {
    input: StructsInputOutput,
    output: StructsInputOutput
}
