/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import cluster from "cluster";
import { cpus } from "os";

import {
    App,
    Handlers,
    GetPokemonSpeciesInput,
    GetPokemonSpeciesOutput,
    Language,
    DoNothingInput,
    DoNothingOutput,
    Socket,
    CheckHealthOutput,
    CheckHealthInput,
    GetServerStatisticsInput,
    GetServerStatisticsOutput,
} from ".";

class HandlerImpl implements Handlers {
    // TODO: implement
    async doNothing(input: DoNothingInput): Promise<DoNothingOutput> {
        return {};
    }
    // TODO: implement
    async checkHealth(input: CheckHealthInput): Promise<CheckHealthOutput> {
        return {};
    }
    // TODO: implement
    async getServerStatistics(
        input: GetServerStatisticsInput
    ): Promise<GetServerStatisticsOutput> {
        return { callsCount: 0 };
    }
    async getPokemonSpecies(
        input: GetPokemonSpeciesInput
    ): Promise<GetPokemonSpeciesOutput> {
        return {
            name: input.name,
            flavorTextEntries: [
                {
                    language: Language.English,
                    flavorText:
                        "When several of these Pokémon gather, their electricity could build and cause lightning storms.",
                },
                {
                    language: Language.Italian,
                    flavorText:
                        "Quando vari Pokémon di questo tipo si radunano, la loro energia può causare forti tempeste.",
                },
                {
                    language: Language.Spanish,
                    flavorText:
                        "Cuando varios de estos Pokémon se juntan, su energía puede causar fuertes tormentas.",
                },
                {
                    language: Language.Japanese,
                    flavorText:
                        "ほっぺたの りょうがわに ちいさい でんきぶくろを もつ。ピンチのときに ほうでんする。",
                },
            ],
        };
    }
}

// Pass the handlers to the App.
const app = new App(new HandlerImpl());
// Start the app 🤘
const numCPUs = cpus().length / 2;
const socket = new Socket("127.0.0.1", 9090);
app.start(socket);

// TODO: This part should be abstracted out and done directly in Rust.
// We could take an optional number of workers and the socket as input
// of the App.start() method.
if (cluster.isPrimary) {
    // Fork workers.
    for (let i = 0; i < numCPUs; i++) {
        cluster.fork();
    }
}
