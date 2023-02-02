import cluster from 'cluster';
import https from "https";
import { cpus } from 'os';
import { text } from 'stream/consumers';

import { JsApp, Handlers, GetPokemonSpeciesInput, GetPokemonSpeciesOutput, Language, DoNothingInput, DoNothingOutput, JsSocket } from ".";

async function get(requestOptions: string): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const request = https.get(requestOptions, (response) => {
      if (
        response.statusCode === undefined ||
        response.statusCode < 200 ||
        response.statusCode > 299
      ) {
        reject(new Error("Non-2xx status code: " + response.statusCode));
      }

      const body = new Array<Buffer | string>();

      response.on("data", (chunk) => body.push(chunk));
      response.on("end", () => resolve(body.join("")));
    });

    request.on("error", (err) => reject(err));
    request.end();
  });
}

// setInterval(() => {
//   console.log('The event loop is not blocked 👌');
// }, 1000);

class HandlerImpl implements Handlers {
  async doNothing(input: DoNothingInput): Promise<DoNothingOutput> {
    return {}
  }
  async getPokemonSpecies(input: GetPokemonSpeciesInput): Promise<GetPokemonSpeciesOutput> {
    // This is an operation handler in JavaScript
    // const text = await get(
    //   "https://people.sc.fsu.edu/~jburkardt/data/csv/snakes_count_10000.csv"
    // );
    return {
      name: input.name,
      flavorTextEntries: [
        {
          language: Language.English,
          flavorText: "When several of these Pokémon gather, their electricity could build and cause lightning storms."
        },
        {
          language: Language.Italian,
          flavorText: "Quando vari Pokémon di questo tipo si radunano, la loro energia può causare forti tempeste."
        },
        {
          language: Language.Spanish,
          flavorText: "Cuando varios de estos Pokémon se juntan, su energía puede causar fuertes tormentas."
        },
        {
          language: Language.Japanese,
          flavorText: "ほっぺたの りょうがわに ちいさい でんきぶくろを もつ。ピンチのときに ほうでんする。"
        }
      ]
    }
  }
}

// Pass the handlers to the SURF
const app = new JsApp(new HandlerImpl());
// Start the app 🤘
const numCPUs = cpus().length / 2;
const socket = new JsSocket("127.0.0.1", 9090);
app.start(socket.tryClone());

if (cluster.isPrimary) {
  // Fork workers.
  for (let i = 0; i < numCPUs; i++) {
    cluster.fork();
  }
}
