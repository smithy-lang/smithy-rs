$version: "2"

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
