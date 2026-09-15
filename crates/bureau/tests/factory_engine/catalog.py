"""SDK catalogue projections over already pinned offline fixture files."""
import json
from pathlib import Path


def policy(config):
    assert set(config["mcpServers"]) == {"bureau-io"}
    broker = config["mcpServers"]["bureau-io"]
    assert set(broker) == {"type", "command", "args", "env", "tools"}
    assert broker["type"] == "stdio"
    methods = {"get_step_context", "publish_result"}
    assert set(broker["tools"]) == methods and len(broker["tools"]) == 2
    assert broker["args"] == ["mcp", "serve"]
    expected_command = Path.cwd().parents[2].joinpath("bureau-executable").read_text()
    assert Path(broker["command"]).resolve() == Path(expected_command)
    assert config["envValueMode"] == "direct"
    actual = {name for name in config["availableTools"] if not name.startswith("builtin:")}
    expected = {f"bureau-io/{name}" for name in methods} if config["requestExtensions"] else set()
    assert actual == expected, "broker selectors must name exactly two methods, not aliases or wildcards"


def initialize(config, session):
    policy(config)
    path = session / "fixture-plugin-roots.json"
    if "pluginDirectories" in config:
        path.write_text(json.dumps(config["pluginDirectories"]))
    else:
        assert path.exists(), "resume must preserve the original activation snapshot"


def plugins(session):
    for directory in json.loads((session / "fixture-plugin-roots.json").read_text()):
        path = Path(directory)
        yield json.loads((path / "plugin.json").read_text())["name"], path


def agents(config, session, mode):
    values = [{"id": item["name"], "name": item["name"],
               "displayName": item["name"], "description": "Offline inline role",
               "mcpServers": item.get("mcpServers"), "skills": item.get("skills")}
              for item in config.get("customAgents", [])]
    for plugin, directory in plugins(session):
        for path in directory.joinpath("agents").glob("*.md"):
            name = path.name.removesuffix(".agent.md").removesuffix(".md")
            identifier = f"{plugin}:{name}"
            values.append({"id": identifier, "name": identifier, "displayName": name,
                           "description": "Offline plugin role", "source": "plugin",
                           "path": str(path), "mcpServers": {},
                           "skills": ["other" if mode == "plugin-bad-binding" else "review"]})
    return {"agents": values}


def skills(session, mode):
    values = []
    for plugin, directory in plugins(session):
        for path in directory.joinpath("skills").glob("*/SKILL.md"):
            name = path.parent.name
            value = {"name": name, "commandName": f"{plugin}:{name}",
                     "description": "Offline pinned skill", "source": "plugin",
                     "userInvocable": mode != "plugin-preload", "enabled": True,
                     "path": str(path), "pluginName": plugin}
            if mode == "plugin-preload":
                del value["commandName"]
            values.append(value)
    return {"skills": values}


def commands(session, mode):
    return {"commands": [
        {"name": item["commandName"], "description": item["description"], "kind": "skill",
         "allowDuringAgentExecution": False}
        for item in skills(session, mode)["skills"] if item["userInvocable"] and item["enabled"]
    ]}

def mcp(session, mode):
    counter = session / "fixture-mcp-polls"
    previous = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(previous + 1))
    status = "pending" if mode == "mcp-pending" and previous == 0 else "connected"
    if mode == "mcp-not-configured":
        status = "not_configured"
    return {"servers": [{"name": "bureau-io", "status": status}]}


def detail(state):
    attempt = state["attempt"]
    return {"runId": "known-offline-run", "factoryName": "offline-factory",
            "description": "Offline lifecycle fixture", "status": state["status"],
            "revision": attempt, "createdAt": 1, "startedAt": 1, "updatedAt": 2,
            "completedAt": None, "currentPhase": None, "declaredPhaseCount": 0,
            "liveAgentCount": 0, "totalSpawnedAgentCount": attempt,
            "consumed": {"activeMs": 100 * attempt, "subagents": attempt,
                         "nanoAiu": 1_000_000_000 * attempt},
            "declaredLimits": {}, "approved": {}, "observedAt": 2,
            "activeSegmentStartedAt": None, "terminal": None,
            "canResume": state["status"] in ("paused", "halted", "error"),
            "phases": [], "agents": [], "progress": {
                "records": [], "oldestSeq": None, "newestSeq": None,
                "hasMoreOlder": False, "hasMoreNewer": False, "revision": attempt}}
