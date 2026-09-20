import { runReview } from "../provider/review.mjs";
import { context } from "./provider-context.mjs";

// A result-shape fixture only: no SDK import, registration or real child call.
process.stdout.write(JSON.stringify(await runReview(context())));
