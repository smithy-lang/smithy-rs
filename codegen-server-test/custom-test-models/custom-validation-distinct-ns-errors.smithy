$version: "2.0"

namespace com.custom.errors

use smithy.framework.rust#validationException
use smithy.framework.rust#validationFieldList
use smithy.framework.rust#validationFieldMessage
use smithy.framework.rust#validationFieldName
use smithy.framework.rust#validationMessage

/// Custom validation exception living in a namespace DISTINCT from the
/// service's namespace. Used to verify assumption B5: does the wire
/// discriminator carry this shape's own namespace?
@error("client")
@httpError(400)
@validationException
structure DistinctNsValidationException {
    @required
    @validationMessage
    customMessage: String

    @validationFieldList
    customFieldList: CustomValidationFieldList
}

structure CustomValidationField {
    @required
    @validationFieldName
    customFieldName: String

    @required
    @validationFieldMessage
    customFieldMessage: String
}

list CustomValidationFieldList {
    member: CustomValidationField
}
