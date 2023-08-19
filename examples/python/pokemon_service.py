#!/usr/bin/env python3
# -*- coding: utf-8 -*-
#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import argparse
import logging
import random
from dataclasses import dataclass
from threading import Lock
from typing import Any, AsyncIterator, Awaitable, Callable, Dict, List, Optional

from pokemon_service_server_sdk import App
from pokemon_service_server_sdk.aws_lambda import LambdaContext
from pokemon_service_server_sdk.error import (
    InvalidPokeballError,
    MasterBallUnsuccessful,
    ResourceNotFoundException,
    UnsupportedRegionError,
    StorageAccessNotAuthorized,
)
from pokemon_service_server_sdk.input import (
    GetStorageInput,
    CapturePokemonInput,
    CheckHealthInput,
    DoNothingInput,
    GetPokemonSpeciesInput,
    GetServerStatisticsInput,
    StreamPokemonRadioInput,
)
from pokemon_service_server_sdk.logging import TracingHandler
from pokemon_service_server_sdk.middleware import MiddlewareException, Request, Response
from pokemon_service_server_sdk.model import (
    CaptureEvent,
    CapturePokemonEvents,
    FlavorText,
    Language,
)
from pokemon_service_server_sdk.output import (
    GetStorageOutput,
    CapturePokemonOutput,
    CheckHealthOutput,
    DoNothingOutput,
    GetPokemonSpeciesOutput,
    GetServerStatisticsOutput,
    StreamPokemonRadioOutput,
)
from pokemon_service_server_sdk.tls import TlsConfig
from pokemon_service_server_sdk.types import ByteStream

# Logging can bee setup using standard Python tooling. We provide
# fast logging handler, Tracingandler based on Rust tracing crate.
logging.basicConfig(
    handlers=[
        TracingHandler(
            level=logging.DEBUG,
            format="pretty",  # You can also use "json" or "compact" (default)
        ).handler()
    ]
)


class SafeCounter:
    def __init__(self) -> None:
        self._val = 0
        self._lock = Lock()

    def increment(self) -> None:
        with self._lock:
            self._val += 1

    def value(self) -> int:
        with self._lock:
            return self._val


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
# Synchronization:
#   Instance of `Context` class will be cloned for every worker and all state kept in `Context`
#   will be specific to that process. There is no protection provided by default,
#   it is up to you to have synchronization between processes.
#   If you really want to share state between different processes you need to use `multiprocessing` primitives:
#   https://docs.python.org/3/library/multiprocessing.html#sharing-state-between-processes
@dataclass
class Context:
    # Inject Lambda context if service is running on Lambda
    # NOTE: All the values that will be injected by the framework should be wrapped with `Optional`
    lambda_ctx: Optional[LambdaContext] = None

    # In our case it simulates an in-memory database containing the description of Pikachu in multiple
    # languages.
    _pokemon_database = {
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
            ),
        ]
    }
    _calls_count = SafeCounter()
    _radio_database = [
        "https://ia800107.us.archive.org/33/items/299SoundEffectCollection/102%20Palette%20Town%20Theme.mp3",
        "https://ia600408.us.archive.org/29/items/PocketMonstersGreenBetaLavenderTownMusicwwwFlvtoCom/Pocket%20Monsters%20Green%20Beta-%20Lavender%20Town%20Music-%5Bwww_flvto_com%5D.mp3",
    ]

    def get_pokemon_description(self, name: str) -> Optional[List[FlavorText]]:
        return self._pokemon_database.get(name)

    def increment_calls_count(self) -> None:
        self._calls_count.increment()
        return None

    def get_calls_count(self) -> int:
        return self._calls_count.value()

    def get_random_radio_stream(self) -> str:
        return random.choice(self._radio_database)


###########################################################
# Entrypoint
###########################################################
# Get an App instance.
app: "App[Context]" = App()
# Register the context.
app.context(Context())


###########################################################
# Middleware
############################################################
# Middlewares are sync or async function decorated by `@app.middleware`.
# They are executed in order and take as input the HTTP request object and
# the handler or the next middleware in the stack.
# A middleware should return a `Response`, either by calling `next` with `Request`
# to get `Response` from the handler or by constructing `Response` by itself.
# It can also modify the `Request` before calling `next` or it can also modify
# the `Response` returned by the handler.
# It can also raise an `MiddlewareException` with custom error message and HTTP status code,
# any other raised exceptions will cause an internal server error response to be returned.

# Next is either the next middleware in the stack or the handler.
Next = Callable[[Request], Awaitable[Response]]


# This middleware checks the `Content-Type` from the request header,
# logs some information depending on that and then calls `next`.
@app.middleware
async def check_content_type_header(request: Request, next: Next) -> Response:
    content_type = request.headers.get("content-type")
    if content_type == "application/json":
        logging.debug("found valid `application/json` content type")
    else:
        logging.warning(
            "invalid content type %s, dumping headers: %s",
            content_type,
            request.headers.items(),
        )
    return await next(request)


# This middleware adds a new header called `x-amzn-answer` to the
# request. We expect to see this header to be populated in the next
# middleware.
@app.middleware
async def add_x_amzn_answer_header(request: Request, next: Next) -> Response:
    request.headers["x-amzn-answer"] = "42"
    logging.debug("setting `x-amzn-answer` header to 42")
    return await next(request)


# This middleware checks if the header `x-amzn-answer` is correctly set
# to 42, otherwise it returns an exception with a set status code.
@app.middleware
async def check_x_amzn_answer_header(request: Request, next: Next) -> Response:
    # Check that `x-amzn-answer` is 42.
    if request.headers.get("x-amzn-answer") != "42":
        # Return an HTTP 401 Unauthorized if the content type is not JSON.
        raise MiddlewareException("Invalid answer", 401)
    return await next(request)


###########################################################
# App handlers definition
###########################################################
# DoNothing operation used for raw benchmarking.
@app.do_nothing
def do_nothing(_: DoNothingInput) -> DoNothingOutput:
    return DoNothingOutput()


# Retrieves the user's storage.
@app.get_storage
def get_storage(input: GetStorageInput) -> GetStorageOutput:
    logging.debug("attempting to authenticate storage user")

    # We currently only support Ash and he has nothing stored
    if input.user != "ash" or input.passcode != "pikachu123":
        logging.debug("authentication failed")
        raise StorageAccessNotAuthorized()

    return GetStorageOutput([])


# Get the translation of a Pokémon specie or an error.
@app.get_pokemon_species
def get_pokemon_species(
    input: GetPokemonSpeciesInput, context: Context
) -> GetPokemonSpeciesOutput:
    if context.lambda_ctx is not None:
        logging.debug(
            "lambda Context: %s",
            dict(
                request_id=context.lambda_ctx.request_id,
                deadline=context.lambda_ctx.deadline,
                invoked_function_arn=context.lambda_ctx.invoked_function_arn,
                function_name=context.lambda_ctx.env_config.function_name,
                memory=context.lambda_ctx.env_config.memory,
                version=context.lambda_ctx.env_config.version,
            ),
        )
    context.increment_calls_count()
    flavor_text_entries = context.get_pokemon_description(input.name)
    if flavor_text_entries:
        logging.debug("total requests executed: %s", context.get_calls_count())
        logging.info("found description for Pokémon %s", input.name)
        return GetPokemonSpeciesOutput(
            name=input.name, flavor_text_entries=flavor_text_entries
        )
    else:
        logging.warning("description for Pokémon %s not in the database", input.name)
        raise ResourceNotFoundException("Requested Pokémon not available")


# Get the number of requests served by this server.
@app.get_server_statistics
def get_server_statistics(
    _: GetServerStatisticsInput, context: Context
) -> GetServerStatisticsOutput:
    calls_count = context.get_calls_count()
    logging.debug("the service handled %d requests", calls_count)
    return GetServerStatisticsOutput(calls_count=calls_count)


# Run a shallow health check of the service.
@app.check_health
def check_health(_: CheckHealthInput) -> CheckHealthOutput:
    return CheckHealthOutput()


###########################################################
# Event streams
############################################################
# An event stream is an abstraction that allows multiple messages to be sent asynchronously
# between a client and server. Event streams support both duplex and simplex streaming.
# You can find more details about event streaming in Smithy Spec:
# https://smithy.io/2.0/spec/streaming.html#event-streams
#
# Event streams modeled as asynchronous Python generators, that means:
# For receiving, you can use `async for` to iterate incoming events from the client and you can
# `break` from the loop if you want to stop receiving events.
# For sending, you can use `yield` to sent an event to the client and you can `return` from your
# generator function to stop `yield`ing events.
#
# Event streams also has a concept of "Modeled Errors" and those events are considered as
# terminal errors and they stop the event stream. They are modeled as exceptions in Python.
# Incoming event streams can raise modeled exceptions to terminate the event stream and you can
# catch those errors by wrapping incoming streams in a `try-except` block, and you can also raise
# a modeled error to terminate event stream.
# See Smithy Spec for more information about modeled errors:
# https://smithy.io/2.0/spec/streaming.html#modeled-errors-in-event-streams
#
# Depending on your use case, your functions usually should look like:
#
# Receiving only:
# ```python
# def receiving(input: Input) -> Output:
#     # Initial message handling...
#
#     async def events(input):
#         try:
#             async for event in input.events:
#                 # Handle incoming `event`...
#                 # `input.events` will gracefully stop if client stops sending events,
#                 # you can also `break` if you want to stop receiving
#         except YourModeledError as err:
#             # Handle modeled error...
#             # The stream is terminated
#
#     # Return immediately and let your generator run in background,
#     # you can also have initial messages and send them here
#     return Output(events=events(input), initial="value")
# ```
#
# Sending only:
# ```python
# def sending(input: Input) -> Output:
#     # Initial message handling...
#
#     async def events(input):
#         while True:
#             # You can send values by `yield`ing them...
#             yield some_value
#
#             # You can just `break` the loop to stop sending events gracefully
#             if some_cond:
#                 break
#
#             # You can also stop sending events by raising modeled errors
#             if some_other_cond:
#                 raise YourModeledError(...)
#
#     # Return immediately and let your generator run in background,
#     # you can also have initial messages and send them here
#     return Output(events=events(input), initial="value")
# ```
#
# Both receiving and sending:
# ```python
# def bidirectional(input: Input) -> Output:
#     # Initial message handling...
#
#     async def events(input):
#         try:
#             async for event in input.events:
#                 # Handle incoming `event`...
#                 # `input.events` will gracefully stop if client stops sending events,
#                 # you can also `break` if you want to stop receiving
#
#                 # Send some event to the client by `yield`ing them, or stop sending events
#                 # by `break`ing the loop or raising modeled errors
#                 yield outgoing
#         except YourModeledError as err:
#             # Handle modeled error...
#             # The stream is terminated
#
#     # Return immediately and let your generator run in background,
#     # you can also have initial messages and send them here
#     return Output(events=events(input), initial="value")
# ```
#
# You can see an example implementation of a duplex streaming for capturing a Pokemon,
# you can find Smithy model in `codegen-server-test/python/model/pokemon.smithy`.
@app.capture_pokemon
def capture_pokemon(input: CapturePokemonInput) -> CapturePokemonOutput:
    # You can have initial messages that provides an opportunity for a client or server to
    # provide metadata about an event stream before transmitting events.
    # See Smithy spec for more details: https://smithy.io/2.0/spec/streaming.html#initial-messages
    if input.region != "Kanto":
        raise UnsupportedRegionError(input.region)

    # Here is your event stream, which is an `async` Python function that can `await` on incoming
    # events and `yield` to sent an event
    async def events(input: CapturePokemonInput) -> AsyncIterator[CapturePokemonEvents]:
        # We're wrapping incoming event stream with a `try-catch` block to catch modeled errors
        try:
            # Asynchronously iterate through incoming events
            async for incoming in input.events:
                # `incoming` is an union of all possible non-error types from your Smithy model.
                # You can use `is_{variant}` and `as_{variant}` methods to inspect incoming message
                if incoming.is_event():
                    event = incoming.as_event()
                    payload = event.payload
                    # You can ignore incoming messages just by `continue`ing the loop
                    if not payload:
                        logging.debug("no payload provided, ignoring the event")
                        continue

                    name = payload.name or "<unknown>"
                    pokeball = payload.pokeball or "<unknown>"
                    # You can terminate the stream by raising modeled errors
                    if pokeball not in ["Master Ball", "Fast Ball"]:
                        raise InvalidPokeballError(pokeball)

                    # Outgoing type is also an union of all possible non-error types from
                    # your Smithy model. You can construct the variant you want by calling
                    # constructor functions for your variant:
                    outgoing_event = CapturePokemonEvents.event(
                        CaptureEvent(
                            name=name,
                            captured=random.choice([True, False]),
                            shiny=random.choice([True, False]),
                        )
                    )
                    # You can `yield` your outgoing type to sent an event to the client
                    yield outgoing_event
                else:
                    # You can stop receiving incoming events by just `break`ing the loop
                    logging.error("unknown event")
                    break
        # You can catch modeled errors and act accordingly, they will terminate the event stream
        except MasterBallUnsuccessful as err:
            logging.error("masterball unsuccessful: %s", err)

        # Here event stream is going to be completed because we stopped receiving events and we'll
        # no longer `yield` new values and this asynchronous Python generator will end here
        logging.debug("done")

    # You should immediately return your output here, your asynchronous Python generator will
    # run in background without blocking your threads.
    return CapturePokemonOutput(events=events(input))


# Stream a random Pokémon song.
@app.stream_pokemon_radio
async def stream_pokemon_radio(
    _: StreamPokemonRadioInput, context: Context
) -> StreamPokemonRadioOutput:
    import aiohttp

    radio_url = context.get_random_radio_stream()
    logging.info("random radio URL for this stream is %s", radio_url)
    async with aiohttp.ClientSession() as session:
        async with session.get(radio_url) as response:
            data = ByteStream(await response.read())
        logging.debug("successfully fetched radio url %s", radio_url)
    return StreamPokemonRadioOutput(data=data)


###########################################################
# Run the server.
###########################################################
def main() -> None:
    parser = argparse.ArgumentParser(description="PokémonService")
    parser.add_argument("--enable-tls", action="store_true")
    parser.add_argument("--tls-key-path")
    parser.add_argument("--tls-cert-path")
    args = parser.parse_args()

    config: Dict[str, Any] = dict(workers=1)
    if args.enable_tls:
        config["tls"] = TlsConfig(
            key_path=args.tls_key_path,
            cert_path=args.tls_cert_path,
        )

    app.run(**config)


if __name__ == "__main__":
    main()
