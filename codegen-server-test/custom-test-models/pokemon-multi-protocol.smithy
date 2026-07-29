$version: "2"

namespace com.aws.example.multi

use aws.protocols#restXml
use com.aws.example#PokemonService
use smithy.protocols#rpcv2Cbor

// Exercise server-side multi-protocol generation without changing the existing Pokémon service.
apply PokemonService @restXml
apply PokemonService @rpcv2Cbor
