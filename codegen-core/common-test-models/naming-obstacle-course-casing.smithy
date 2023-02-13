$version: "1.0"
namespace casing

use aws.protocols#awsJson1_1

// TODO(https://github.com/awslabs/smithy-rs/issues/2340): The commented part of the model breaks the generator in a
// miriad of ways. Any solution to the linked issue must address this.

/// Confounds model generation machinery with lots of problematic casing
@awsJson1_1
service ACRONYMInside_Service {
    operations: [
        DoNothing,
    //    ACRONYMInside_Op
    //    ACRONYM_InsideOp
    ]
}

operation DoNothing {}

// operation ACRONYMInside_Op {
//     input: Input,
//     output: Output,
//     errors: [Error],
// }

// operation ACRONYM_InsideOp {
//     input: Input,
//     output: Output,
//     errors: [Error],
// }

// structure Input {
//     ACRONYMInside_Member: ACRONYMInside_Struct,
//     ACRONYM_Inside_Member: ACRONYM_InsideStruct,
//     ACRONYM_InsideMember: ACRONYMInsideStruct
// }

// structure Output {
//     ACRONYMInside_Member: ACRONYMInside_Struct,
//     ACRONYM_Inside_Member: ACRONYM_InsideStruct,
//     ACRONYM_InsideMember: ACRONYMInsideStruct
// }

// @error("client")
// structure Error {
//     ACRONYMInside_Member: ACRONYMInside_Struct,
//     ACRONYM_Inside_Member: ACRONYM_InsideStruct,
//     ACRONYM_InsideMember: ACRONYMInsideStruct
// }

// structure ACRONYMInside_Struct {
//     ACRONYMInside_Member: ACRONYM_InsideStruct,
//     ACRONYM_Inside_Member: Integer,
// }

// structure ACRONYM_InsideStruct {
//     ACRONYMInside_Member: Integer,
// }

// structure ACRONYMInsideStruct {
//     ACRONYMInside_Member: Integer,
// }
