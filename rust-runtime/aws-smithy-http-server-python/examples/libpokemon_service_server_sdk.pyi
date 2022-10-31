#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

# NOTE: This is manually created to surpass some mypy errors and it is incomplete,
#       in future we will autogenerate correct stubs.

from typing import Any, TypeVar, Callable

F = TypeVar("F", bound=Callable[..., Any])

class App:
    context: Any
    run: Any

    def middleware(self, func: F) -> F: ...
    def do_nothing(self, func: F) -> F: ...
    def get_pokemon_species(self, func: F) -> F: ...
    def get_server_statistics(self, func: F) -> F: ...
    def check_health(self, func: F) -> F: ...
    def stream_pokemon_radio(self, func: F) -> F: ...
