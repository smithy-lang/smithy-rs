$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    version: "2022-01-01",
    operations: [
        PutThing,
    ],
}

@http(method: "POST", uri: "/things")
operation PutThing {
    input: PutThingInput
}

structure PutThingInput {
    @httpQuery("thingId")
    @required
    thingId: String,

    @httpQueryParams
    tags: MapOfSetOfStrings,

    setOfDoubles: SetOfDoubles
}

map MapOfStrings {
    key: String,
    value: String
}

map MapOfListOfStrings {
    key: String,
    value: ListOfStrings
}

list ListOfStrings {
    member: String
}

map MapOfSetOfStrings {
    key: String,
    value: SetOfStrings
}

set SetOfStrings {
    member: String
}

set SetOfDoubles {
    member: Double
}
