#!/usr/bin/env python3
# -*- coding: utf-8 -*-
#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import logging
from dataclasses import dataclass
from typing import List, Optional
import itertools
import threading

from libpokemon_service_sdk.error import \
    ResourceNotFoundException
from libpokemon_service_sdk.input import (
    EmptyOperationInput, GetPokemonSpeciesInput, GetServerStatisticsInput)
from libpokemon_service_sdk.model import FlavorText, Language
from libpokemon_service_sdk.output import (
    EmptyOperationOutput, GetPokemonSpeciesOutput, GetServerStatisticsOutput)
from libpokemon_service_sdk import App


# A slightly more atomic counter using a threading lock.
class FastWriteCounter:
    def __init__(self):
        self._number_of_read = 0
        self._counter = itertools.count()
        self._read_lock = threading.Lock()

    def increment(self):
        next(self._counter)

    def value(self):
        with self._read_lock:
            value = next(self._counter) - self._number_of_read
            self._number_of_read += 1
        return value


###########################################################
# State management
###########################################################
# This context class is used to share data between handlers. It is automatically injected
# inside the `State` object that can be imported from the shared library.
# The `State` object will allow to access to the context class defined below via the `context`
# attribute as well as other information and helpers for the current request such has the
# operation name.
#
# We force the operation handlers to be defined as syncronous or asyncronous functions, taking in
# input the input structure and the state from the shared library and returning the output structure
# or raising one error from the the shared library.
#
# Examples:
#   * def operation(input: OperationInput, state: State) -> OperationOutput
#   * async def operation(input: OperationInput, state: State) -> OperationOutput
#
# NOTE: protection of the data inside the context class is up to the developer
@dataclass
class Context:
    # In our case it simulates an in-memory database containing the description of Pikachu in multiple
    # languages.
    _database = {
        "pikachu": [
            FlavorText(
                flavor_text="""When several of these Pokémon gather, their electricity could build and cause lightning storms.""",
                language=Language.English,
            ),
            FlavorText(
                flavor_text="""Quando vari Pokémon di questo tipo si radunano, la loro energia può causare forti tempeste.""",
                language=Language.Italian,
            ),
            FlavorText(
                flavor_text="""Cuando varios de estos Pokémon se juntan, su energía puede causar fuertes tormentas.""",
                language=Language.Spanish,
            ),
            FlavorText(
                flavor_text="ほっぺたの りょうがわに ちいさい でんきぶくろを もつ。ピンチのときに ほうでんする。",
                language=Language.Japanese,
            )
        ]
    }
    _calls_count = FastWriteCounter()

    def get_pokemon_description(self, name: str) -> Optional[List[FlavorText]]:
        return self._database.get(name)

    def increment_calls_count(self) -> None:
        self._calls_count.increment()

    def get_calls_count(self) -> int:
        return self._calls_count.value()


###########################################################
# Entrypoint
###########################################################
# Get an App instance.
app = App()
# Register the context.
app.context(Context())


###########################################################
# App handlers definition
###########################################################
# Empty operation used for raw benchmarking.
@app.empty_operation
def empty_operation(_: EmptyOperationInput) -> EmptyOperationOutput:
    logging.debug("Running the empty operation")
    return EmptyOperationOutput()


# Get the translation of a Pokémon specie or an error.
@app.get_pokemon_species
def get_pokemon_species(
    input: GetPokemonSpeciesInput, context: Context
) -> GetPokemonSpeciesOutput:
    context.increment_calls_count()
    flavor_text_entries = context.get_pokemon_description(input.name)
    if flavor_text_entries:
        logging.debug("Total requests executed: %s", context.get_calls_count())
        logging.info("Found description for Pokémon %s", input.name)
        return GetPokemonSpeciesOutput(
            name=input.name, flavor_text_entries=flavor_text_entries
        )
    else:
        logging.warning("Description for Pokémon %s not in the database", input.name)
        raise ResourceNotFoundException("Requested Pokémon not available")


# Get the number of requests served by this server.
@app.get_server_statistics
def get_server_statistics(
    _: GetServerStatisticsInput, context: Context
) -> GetServerStatisticsOutput:
    calls_count = context.get_calls_count()
    logging.debug("The service handled %d requests", calls_count)
    return GetServerStatisticsOutput(calls_count=calls_count)

###########################################################
# Run the server.
###########################################################
app.run(workers=1)
