$version: "2"

// Adds AWS JSON 1.0/1.1, REST XML, and rpcv2Cbor to the existing REST JSON Pokemon service.
// The schema HTTP 1.x projection can then exercise the same generated operations under all
// five protocols without duplicating the base service model.

namespace com.aws.example

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor
use com.aws.example#PokemonService

// Extend the service from pokemon.smithy; restJson1 is already declared there.
apply PokemonService @awsJson1_0
apply PokemonService @awsJson1_1
apply PokemonService @restXml
apply PokemonService @rpcv2Cbor
