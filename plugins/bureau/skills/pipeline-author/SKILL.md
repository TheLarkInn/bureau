---
name: pipeline-author
description: Authors and validates bureau repository, role, assignment, and pipeline configuration from an automation goal.
license: MIT
metadata:
  tags: "bureau, pipelines, automation, configuration"
  category: "development"
---

# pipeline-author

Turn an automation goal into reviewed bureau configuration.

1. Read `DESIGN.md`, especially sections 2, 3, 6, 9, 10, and 15.
2. Determine whether config belongs in the work repository's `.bureau/`
   directory or in a separate multi-repository config repository.
3. Reuse installable plugin agents. Do not redeclare agent instructions,
   tools, or models in role YAML.
4. Keep forge filters native and opaque. Never translate them into a bureau
   filter language.
5. Require `work.approval_label` when ADO input must reach an agent requiring
   maintainer trust.
6. Give roles only the credential permissions their step needs.
7. Use deterministic steps for machine checks and ordinary outcome edges for
   routing. Use `type: concurrent` only for independent evidence-only steps.
8. Leave optional limits blank unless the user supplies values. The complete
   run still has bureau's default 24-hour deadline.
9. Run `bureau validate <config-path>` and fix every reported problem.
10. Show the final files and explain the next Git review action. Do not start
    reconciliation from unmerged config.
