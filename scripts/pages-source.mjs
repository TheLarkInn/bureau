const repository = { owner: "TheLarkInn", repo: "bureau" };

export async function pagesSource({ github, context }) {
  if (!/^[0-9a-f]{40}$/u.test(context.sha)) throw new Error("Pages source must be an immutable commit SHA.");
  if (context.eventName === "pull_request") {
    if (context.payload.pull_request?.base.ref !== "main") throw new Error("Site pull requests must target main.");
    return context.sha;
  }
  if (!["push", "workflow_dispatch"].includes(context.eventName)) throw new Error("Unsupported Pages event.");
  if (context.repo.owner !== repository.owner || context.repo.repo !== repository.repo
    || context.ref !== "refs/heads/main") {
    throw new Error("Pages publishing is restricted to this repository's main branch.");
  }
  const { data } = await github.rest.git.getRef({ ...repository, ref: "heads/main" });
  if (data.object.type !== "commit" || data.object.sha !== context.sha) {
    throw new Error("Main advanced or the source is not a commit; rerun Pages on current main.");
  }
  return context.sha;
}
