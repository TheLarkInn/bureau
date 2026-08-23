// The served state for a pipeline whose middle step is a concurrent group.
//
// Generated, not written. `test/fixtures/concurrent-payload.json` is a
// `bureau validate --json` answer; this is what `extension.mjs` builds from
// it — the same layout projector, the same handles, the same relation graph the
// host would serve. `test/statelab.test.mjs` rebuilds it and fails on any
// difference, so it cannot drift from the host.
//
// The 4 host-owned fields (`canvasId`, `instanceId`, `repoRoot`, `dir`) are absent
// on purpose: the fixture takes those from the payload it projects over, so the
// same committed shape renders on every machine.
//
// To regenerate: node test/regenerate-concurrent-state.mjs

export const CONCURRENT_STATE = {
  "pipeline": "review-queue-pipeline",
  "message": "Bureau config view.",
  "status": "Validated",
  "validation": {
    "ok": true,
    "state": "validated",
    "dir": "/repo/.bureau",
    "errors": [],
    "message": null
  },
  "findings": [],
  "findingsByItem": {},
  "findingsByStep": {},
  "generalFindings": [],
  "config": {
    "view": {
      "dir": "/repo/.bureau",
      "repos": [
        {
          "name": "bureau",
          "url": "https://github.com/TheLarkInn/bureau",
          "access": "push",
          "credential": "GH_TOKEN",
          "usedBy": [
            "assignment:review-queue"
          ]
        }
      ],
      "roles": [
        {
          "name": "reviewer",
          "agent": "reviewer",
          "permissions": [
            "repo:read",
            "model:invoke"
          ],
          "usedBy": [
            "pipeline:review-queue-pipeline/read-diff"
          ]
        }
      ],
      "assignments": [
        {
          "name": "review-queue",
          "work": {
            "forge": "github",
            "source": "https://github.com/TheLarkInn/bureau/issues",
            "filter": "is:issue is:open label:review",
            "abortLabel": "bureau:stop",
            "escalateLabel": "bureau:needs-human"
          },
          "repos": [
            "bureau"
          ],
          "primaryRepo": "bureau",
          "pipeline": "review-queue-pipeline",
          "branchPrefix": "bureau/",
          "limits": {
            "maxConcurrent": 1,
            "maxRunsPerHour": null,
            "maxRunsPerDay": null,
            "maxOpenPrs": null,
            "maxCostPerDayUsd": null,
            "maxRunHours": null
          }
        }
      ],
      "pipelines": [
        {
          "name": "review-queue-pipeline",
          "stepCount": 4,
          "kinds": [
            "deterministic",
            "concurrent",
            "agent"
          ],
          "roles": [
            "reviewer"
          ],
          "terminals": [
            "done",
            "escalate"
          ],
          "usedBy": [
            "assignment:review-queue"
          ]
        }
      ],
      "orphans": []
    },
    "layout": {
      "dir": "/repo/.bureau",
      "items": [
        {
          "id": "assignment:review-queue",
          "kind": "assignment",
          "name": "review-queue",
          "row": 0,
          "column": 0,
          "height": 160,
          "x": 0,
          "y": 0
        },
        {
          "id": "pipeline:review-queue-pipeline",
          "kind": "pipeline",
          "name": "review-queue-pipeline",
          "row": 0,
          "column": 1,
          "height": 288,
          "x": 320,
          "y": 0
        },
        {
          "id": "role:reviewer",
          "kind": "role",
          "name": "reviewer",
          "row": 0,
          "column": 2,
          "height": 160,
          "x": 640,
          "y": 0
        },
        {
          "id": "repo:bureau",
          "kind": "repo",
          "name": "bureau",
          "row": 0,
          "column": 3,
          "height": 160,
          "x": 960,
          "y": 0
        }
      ],
      "edges": [
        {
          "id": "pipeline:assignment:review-queue->pipeline:review-queue-pipeline",
          "source": "assignment:review-queue",
          "target": "pipeline:review-queue-pipeline",
          "relation": "pipeline"
        },
        {
          "id": "repo:assignment:review-queue->repo:bureau",
          "source": "assignment:review-queue",
          "target": "repo:bureau",
          "relation": "repo"
        },
        {
          "id": "role:pipeline:review-queue-pipeline->role:reviewer",
          "source": "pipeline:review-queue-pipeline",
          "target": "role:reviewer",
          "relation": "role"
        }
      ]
    },
    "relation": {
      "nodes": [
        {
          "id": "assignment:review-queue",
          "kind": "assignment",
          "name": "review-queue"
        },
        {
          "id": "pipeline:review-queue-pipeline",
          "kind": "pipeline",
          "name": "review-queue-pipeline"
        },
        {
          "id": "role:reviewer",
          "kind": "role",
          "name": "reviewer"
        },
        {
          "id": "repo:bureau",
          "kind": "repo",
          "name": "bureau"
        }
      ],
      "edges": [
        {
          "id": "pipeline:assignment:review-queue->pipeline:review-queue-pipeline",
          "source": "assignment:review-queue",
          "target": "pipeline:review-queue-pipeline",
          "relation": "pipeline"
        },
        {
          "id": "repo:assignment:review-queue->repo:bureau",
          "source": "assignment:review-queue",
          "target": "repo:bureau",
          "relation": "repo"
        },
        {
          "id": "role:pipeline:review-queue-pipeline->role:reviewer",
          "source": "pipeline:review-queue-pipeline",
          "target": "role:reviewer",
          "relation": "role"
        }
      ]
    }
  },
  "pipelines": {
    "review-queue-pipeline": {
      "view": {
        "name": "review-queue-pipeline",
        "steps": [
          {
            "id": "claim",
            "name": "claim",
            "type": "step",
            "kind": "deterministic",
            "order": 0,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "run": "bureau claim"
            }
          },
          {
            "id": "run-checks",
            "name": "run-checks",
            "type": "step",
            "kind": "concurrent",
            "order": 1,
            "fields": {
              "inputsFrom": [
                "claim"
              ],
              "maxAttempts": 1,
              "members": [
                "read-diff",
                "read-tests"
              ],
              "completion": "all",
              "maxConcurrent": 2
            }
          },
          {
            "id": "read-diff",
            "name": "read-diff",
            "type": "step",
            "kind": "agent",
            "order": 2,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "role": "reviewer"
            },
            "parentId": "run-checks"
          },
          {
            "id": "read-tests",
            "name": "read-tests",
            "type": "step",
            "kind": "deterministic",
            "order": 3,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "run": "cargo test --offline"
            },
            "parentId": "run-checks"
          }
        ],
        "terminals": [
          {
            "id": "terminal:done",
            "name": "done",
            "type": "terminal"
          },
          {
            "id": "terminal:escalate",
            "name": "escalate",
            "type": "terminal"
          }
        ],
        "edges": [
          {
            "id": "control:claim:success->run-checks",
            "source": "claim",
            "target": "run-checks",
            "relation": "control",
            "outcome": "success"
          },
          {
            "id": "control:claim:failure->terminal:escalate",
            "source": "claim",
            "target": "terminal:escalate",
            "relation": "control",
            "outcome": "failure"
          },
          {
            "id": "control:run-checks:success->terminal:done",
            "source": "run-checks",
            "target": "terminal:done",
            "relation": "control",
            "outcome": "success"
          },
          {
            "id": "control:run-checks:failure->terminal:escalate",
            "source": "run-checks",
            "target": "terminal:escalate",
            "relation": "control",
            "outcome": "failure"
          },
          {
            "id": "data:claim->run-checks",
            "source": "claim",
            "target": "run-checks",
            "relation": "data"
          }
        ]
      },
      "layout": {
        "name": "review-queue-pipeline",
        "steps": [
          {
            "id": "claim",
            "name": "claim",
            "type": "step",
            "kind": "deterministic",
            "order": 0,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "run": "bureau claim"
            },
            "row": 0,
            "column": 0,
            "x": 0,
            "y": 0
          },
          {
            "id": "run-checks",
            "name": "run-checks",
            "type": "step",
            "kind": "concurrent",
            "order": 1,
            "fields": {
              "inputsFrom": [
                "claim"
              ],
              "maxAttempts": 1,
              "members": [
                "read-diff",
                "read-tests"
              ],
              "completion": "all",
              "maxConcurrent": 2
            },
            "row": 1,
            "column": 0,
            "x": 0,
            "y": 190
          },
          {
            "id": "read-diff",
            "name": "read-diff",
            "type": "step",
            "kind": "agent",
            "order": 2,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "role": "reviewer"
            },
            "parentId": "run-checks",
            "row": 2,
            "column": 0,
            "x": 0,
            "y": 380
          },
          {
            "id": "read-tests",
            "name": "read-tests",
            "type": "step",
            "kind": "deterministic",
            "order": 3,
            "fields": {
              "inputsFrom": [],
              "maxAttempts": 1,
              "run": "cargo test --offline"
            },
            "parentId": "run-checks",
            "row": 2,
            "column": 1,
            "x": 320,
            "y": 380
          }
        ],
        "terminals": [
          {
            "id": "terminal:done",
            "name": "done",
            "type": "terminal",
            "row": 2,
            "column": "terminal",
            "x": 760,
            "y": 380
          },
          {
            "id": "terminal:escalate",
            "name": "escalate",
            "type": "terminal",
            "row": 3,
            "column": "terminal",
            "x": 760,
            "y": 570
          }
        ],
        "edges": [
          {
            "id": "control:claim:success->run-checks",
            "source": "claim",
            "target": "run-checks",
            "relation": "control",
            "outcome": "success",
            "route": "spine"
          },
          {
            "id": "control:claim:failure->terminal:escalate",
            "source": "claim",
            "target": "terminal:escalate",
            "relation": "control",
            "outcome": "failure",
            "route": "exit"
          },
          {
            "id": "data:claim->run-checks",
            "source": "claim",
            "target": "run-checks",
            "relation": "data",
            "route": "data"
          },
          {
            "id": "control:run-checks:success->terminal:done",
            "source": "run-checks",
            "target": "terminal:done",
            "relation": "control",
            "outcome": "success",
            "route": "spine"
          },
          {
            "id": "control:run-checks:failure->terminal:escalate",
            "source": "run-checks",
            "target": "terminal:escalate",
            "relation": "control",
            "outcome": "failure",
            "route": "exit"
          }
        ]
      },
      "handles": {
        "items": {
          "claim": {
            "source": [
              {
                "id": "right:data",
                "side": "right",
                "name": "data"
              },
              {
                "id": "right:failure",
                "side": "right",
                "name": "failure"
              },
              {
                "id": "bottom:success",
                "side": "bottom",
                "name": "success"
              }
            ],
            "target": []
          },
          "run-checks": {
            "source": [
              {
                "id": "right:failure",
                "side": "right",
                "name": "failure"
              },
              {
                "id": "bottom:success",
                "side": "bottom",
                "name": "success"
              }
            ],
            "target": [
              {
                "id": "top:in",
                "side": "top",
                "name": "in"
              },
              {
                "id": "left:in-left",
                "side": "left",
                "name": "in-left"
              }
            ]
          },
          "read-diff": {
            "source": [],
            "target": []
          },
          "read-tests": {
            "source": [],
            "target": []
          },
          "terminal:done": {
            "source": [],
            "target": [
              {
                "id": "top:in",
                "side": "top",
                "name": "in"
              }
            ]
          },
          "terminal:escalate": {
            "source": [],
            "target": [
              {
                "id": "top:in",
                "side": "top",
                "name": "in"
              }
            ]
          }
        },
        "edges": {
          "control:claim:success->run-checks": {
            "source": "bottom:success",
            "target": "top:in"
          },
          "control:claim:failure->terminal:escalate": {
            "source": "right:failure",
            "target": "top:in"
          },
          "data:claim->run-checks": {
            "source": "right:data",
            "target": "left:in-left"
          },
          "control:run-checks:success->terminal:done": {
            "source": "bottom:success",
            "target": "top:in"
          },
          "control:run-checks:failure->terminal:escalate": {
            "source": "right:failure",
            "target": "top:in"
          }
        }
      },
      "containers": [
        {
          "id": "concurrent:run-checks",
          "parent": "run-checks",
          "members": [
            "read-diff",
            "read-tests"
          ],
          "x": 0,
          "y": 190,
          "width": 320,
          "height": 190
        }
      ],
      "summary": {
        "kindCounts": {
          "deterministic": 2,
          "concurrent": 1,
          "agent": 1
        },
        "agentSteps": [
          {
            "name": "read-diff",
            "role": "reviewer",
            "trust": null,
            "ref": "pipeline:review-queue-pipeline/read-diff"
          }
        ]
      },
      "arrangement": {}
    }
  },
  "plan": null,
  "selectedPipeline": {
    "name": "review-queue-pipeline",
    "missing": false
  }
};
