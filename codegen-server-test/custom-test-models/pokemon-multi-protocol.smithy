$version: "2"

namespace com.aws.example.multi

use com.aws.example#PokemonService

// All compatible protocols are now applied directly on the PokemonService in pokemon.smithy:
// @restJson1, @restXml, @rpcv2Cbor
// Note: @awsJson1_0 and @awsJson1_1 are incompatible with @httpPayload + streaming members
// used by CapturePokemon and StreamPokemonRadio operations.
