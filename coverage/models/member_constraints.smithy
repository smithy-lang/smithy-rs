// coverage-service: test.memberconstraints#MemberConstraintsService
// coverage-plugins: server
$version: "2.0"

namespace test.memberconstraints

use aws.protocols#restJson1
use smithy.framework#ValidationException

// Constraint traits applied directly to member shapes (of structures, lists, and maps) are
// extracted into synthetic shapes by ConstrainedMemberTransform; the synthetic shapes land in
// inline modules, exercising RustCrateInlineModuleComposingWriter and the
// getParentAndInlineModuleForConstrainedMember paths.
@restJson1
service MemberConstraintsService {
    version: "1.0"
    operations: [PutMemberConstrained]
}

@http(method: "POST", uri: "/member-constrained")
operation PutMemberConstrained {
    input := {
        @required
        @length(min: 1, max: 16)
        name: String

        @pattern("^[a-z]+$")
        code: String

        names: ConstrainedMemberList

        lookup: ConstrainedValueMap

        keyed: ConstrainedKeyMap
    }
    output := {
        @length(min: 1, max: 32)
        id: String
    }
    errors: [ValidationException]
}

list ConstrainedMemberList {
    @length(min: 1, max: 8)
    member: String
}

map ConstrainedValueMap {
    key: String

    @pattern("^[0-9]+$")
    value: String
}

map ConstrainedKeyMap {
    @length(min: 2, max: 4)
    key: String

    value: String
}
