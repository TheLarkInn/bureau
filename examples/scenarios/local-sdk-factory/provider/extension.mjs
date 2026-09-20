import { readFile } from "node:fs/promises";
import { defineFactory, joinSession } from "@github/copilot-sdk/extension";
import { runReview } from "./review.mjs";

const meta = JSON.parse(await readFile(new URL("./factory.json", import.meta.url), "utf8"));
const review = defineFactory({ meta, run: runReview });

await joinSession({ factories: [review] });
