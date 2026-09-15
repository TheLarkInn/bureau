#!/usr/bin/python3
"""Offline Content-Length peer. It never launches Copilot, JavaScript, or a model."""
import json
import os
from pathlib import Path
import sqlite3
import sys
import time
sys.dont_write_bytecode = True
import authentication
import behavior
import catalog

HOME = Path(os.environ["COPILOT_HOME"])
SESSION = None
CONFIG = None
MODE = Path(__file__).parent.joinpath("mode").read_text()


def trace(value):
    with (HOME / "trace.jsonl").open("a", encoding="utf-8") as output:
        output.write(json.dumps(authentication.redacted(value)) + "\n")


def send(value):
    body = json.dumps(value, ensure_ascii=False).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()


def receive():
    header = sys.stdin.buffer.readline()
    if not header:
        raise EOFError()
    assert header.startswith(b"Content-Length: ")
    size = int(header.split(b":", 1)[1].strip())
    assert sys.stdin.buffer.readline() == b"\r\n"
    value = json.loads(sys.stdin.buffer.read(size))
    trace(value)
    return value


def response(request, value):
    send({"jsonrpc": "2.0", "id": request["id"], "result": value})


def notify(kind, data):
    send({"jsonrpc": "2.0", "method": "session.event",
          "params": {"sessionId": SESSION.name, "event": {"type": kind, "data": data}}})


def save(value):
    with sqlite3.connect(SESSION / "session.db") as connection:
        connection.execute("CREATE TABLE IF NOT EXISTS fixture (id INTEGER PRIMARY KEY, data TEXT)")
        connection.execute("INSERT OR REPLACE INTO fixture VALUES (1, ?)", (json.dumps(value),))


def saved():
    with sqlite3.connect(SESSION / "session.db") as connection:
        row = connection.execute("SELECT data FROM fixture WHERE id=1").fetchone()
    return json.loads(row[0])


def launch_callback(params):
    identity = f"session:{SESSION.name}:{SESSION.name}"
    entrypoint = SESSION / "extensions" / SESSION.name / "extension.mjs"
    send({"jsonrpc": "2.0", "id": 8000, "method": "extensionLaunchProvider.resolve",
          "params": {"id": "project:unapproved", "name": "unapproved",
                     "modulePath": str(entrypoint), "source": "project"}})
    denied = receive()
    assert denied["id"] == 8000 and denied["result"] == {}
    send({"jsonrpc": "2.0", "id": 8001, "method": "extensionLaunchProvider.resolve",
          "params": {"id": identity, "name": SESSION.name,
                     "modulePath": str(entrypoint), "source": "session"}})
    allowed = receive()
    assert allowed["id"] == 8001
    profile = allowed["result"]["launch"]
    assert profile["env"]["EXTENSION_PATH"] == str(entrypoint)
    assert len(profile["args"]) == 1
    assert profile["args"][0].endswith("/preloads/extension_bootstrap.mjs")
    trace(authentication.descendant("extension", profile["env"]))
    trace({"callback": "extensionLaunchProvider.resolve", "result": allowed["result"]})


def initialize(request):
    global SESSION, CONFIG
    params = request["params"]
    SESSION = HOME / "session-state" / params["sessionId"]
    CONFIG = params
    authentication.session(params)
    catalog.initialize(params, SESSION)
    assert Path.cwd().parent.joinpath("artifacts").is_dir()
    assert params["requestPermission"] is True
    assert params["envValueMode"] == "direct"
    assert params["mcpServers"]["bureau-io"]["args"] == ["mcp", "serve"]
    assert Path(params["mcpServers"]["bureau-io"]["command"]).is_absolute()
    trace(authentication.descendant("mcp", params["mcpServers"]["bureau-io"]["env"]))
    trace(authentication.descendant("shell", {}))
    if request["method"] == "session.create":
        assert params["trustWorkingDirectory"] == "session"
        assert params["enableConfigDiscovery"] is False
        assert "pluginDirectories" in params
        (SESSION / "workspace.yaml").write_text(json.dumps({
            "id": params["sessionId"], "cwd": params["workingDirectory"]}))
        save({"attempt": 0, "status": "pending", "mode": None})
    else:
        assert not any(key in params for key in (
            "workingDirectory", "enableConfigDiscovery", "pluginDirectories", "trustWorkingDirectory"))
        assert (SESSION / "session.db").exists()
    if params["requestExtensions"]:
        launch_callback(params)
    if MODE == "bootstrap-pause" and request["method"] == "session.create":
        Path.cwd().parent.joinpath("PAUSE").write_text("pause before native admission")
    response(request, {"sessionId": "unrelated-session" if MODE == "wrong-session" else params["sessionId"]})


def result_document(state):
    if state["mode"] == "null-result":
        return None
    value = {"schema": "v2", "outcome": "success", "outputs": {"answer": 42},
             "artifacts": [], "trust": "trusted", "message": "offline factory result"}
    if state["mode"] == "credential-artifact":
        secret = CONFIG["gitHubToken"]
        Path.cwd().joinpath("credential.txt").write_text(secret)
        value["outputs"]["credential"] = secret
        value["artifacts"] = [{"name": "credential.txt", "path": "credential.txt"}]
    if state["mode"] == "artifact-escape":
        Path.cwd().parent.joinpath("outside.txt").write_text("outside the factory workspace")
        value["artifacts"] = [{"name": "outside.txt", "path": "../outside.txt"}]
    return value


def envelope(state):
    value = {"runId": "known-offline-run", "attempt": state["attempt"], "status": state["status"]}
    if state["status"] == "completed":
        value["result"] = result_document(state)
    failure = behavior.failure(state)
    if failure:
        value["failure"] = failure
    return value


def start(request):
    params = request["params"]
    assert set(params) == {"sessionId", "name", "args", "options"}
    assert params["name"] == "offline-factory"
    state = {"attempt": 1, "status": "running", "mode": params["args"]["mode"]}
    save(state)
    notify("factory.run_started", {"runId": "known-offline-run",
                                  "factoryName": "offline-factory", "attempt": 1})
    if state["mode"] == "ambiguous":
        sys.exit(0)
    response(request, envelope(state))


def observe(request):
    state = saved()
    if MODE == "delayed-observation-pause" and state["status"] == "running" and state["attempt"] == 1:
        Path.cwd().parent.joinpath("PAUSE").write_text("pause while authoritative read is pending")
        control_request = receive()
        assert control_request["method"] == "session.factory.pause"
        control(control_request)
        state = saved()
    behavior.before_observe(request, state, SESSION, CONFIG, notify, receive, response)
    if state["status"] == "running":
        if state["mode"] in ("pause", "cancel", "pause-hang", "resume-wrong-id", "pause-then-cancel", "pause-cancel-race", "plugin-pause", "unowned-cancel") and state["attempt"] == 1:
            marker = "CANCEL" if state["mode"] == "cancel" else "PAUSE"
            Path.cwd().parent.joinpath(marker).write_text("offline control")
            behavior.restore_effect(state["mode"], state["attempt"])
        else:
            state["status"] = "completed"
    save(state)
    value = envelope(state)
    if MODE == "pause-cancel-race" and state["status"] == "paused":
        Path.cwd().parent.joinpath("CANCEL").write_text("cancel between result and detail")
        trace({"race": "paused-result-before-cancel"})
    if MODE == "wrong-run":
        value["runId"] = "unrelated-run"
    response(request, value)


def detail(request):
    state = saved()
    if (MODE == "pause-cancel-race" and state["status"] == "paused"
            and Path.cwd().parent.joinpath("CANCEL").exists()):
        control_request = receive()
        assert control_request["method"] == "session.factory.cancel"
        control(control_request)
        state = saved()
    value = catalog.detail(state)
    if MODE in ("terminal-status-conflict", "unrequested-cancel"):
        value["status"] = "cancelled"
    if MODE == "missing-accounting":
        del value["consumed"]
    response(request, value)


def control(request):
    state = saved()
    if MODE == "unowned-cancel" and request["method"].endswith(".cancel"):
        return response(request, envelope(state))
    if MODE == "pause-hang":
        time.sleep(20)
    state["status"] = "paused" if request["method"].endswith(".pause") else "cancelled"
    if MODE == "pause-then-cancel" and state["status"] == "paused":
        Path.cwd().parent.joinpath("CANCEL").write_text("cancel while pause is settling")
    save(state)
    response(request, envelope(state))


def resume(request):
    assert set(request["params"]) == {"sessionId", "runId"}
    assert request["params"]["runId"] == "known-offline-run"
    state = saved()
    assert state["status"] in ("paused", "halted", "error")
    behavior.resumed_effect(state["mode"])
    state["attempt"] += 1
    state["status"] = "running"
    save(state)
    value = envelope(state)
    if MODE == "resume-wrong-id":
        value["runId"] = "unrelated-resume"
    response(request, {"factoryName": "offline-factory", "run": value})

def metadata():
    if MODE == "null-tool-metadata":
        return {"tools": None}
    tools = []
    for name in CONFIG["availableTools"]:
        if "/" in name:
            server, tool = name.split("/")
            tools.append({"name": server + "-" + tool, "namespacedName": name,
                          "mcpServerName": "Offline display label" if MODE == "mcp-display-label" else server,
                          "mcpToolName": tool, "deferLoading": True})
        else:
            tools.append({"name": name.removeprefix("builtin:"), "description": "Offline builtin"})
    if MODE == "unapproved-tool":
        tools.append({"name": "bash"})
    if MODE == "unoffered-builtins":
        tools = [tool for tool in tools if "mcpServerName" in tool]
    return {"tools": tools}


def dispatch(request):
    method = request["method"]
    if method in ("session.create", "session.resume"):
        return initialize(request)
    if method == "session.factory.run":
        return start(request)
    if method == "session.factory.resume":
        return resume(request)
    if method == "session.factory.getRun":
        return observe(request)
    if method == "session.factory.getRunDetail":
        return detail(request)
    if method in ("session.factory.pause", "session.factory.cancel"):
        return control(request)
    if method == "session.mcp.list":
        return response(request, catalog.mcp(SESSION, MODE))
    if method == "session.factory.listRuns" and MODE in ("ineligible-session", "credential-rpc-error"):
        return send({"jsonrpc": "2.0", "id": request["id"], "error": {
            "code": -32601, "message": "offline session is not eligible for factories" + (
                CONFIG["gitHubToken"] if MODE == "credential-rpc-error" else ""),
            "data": {"code": "agent_factories_unavailable"}}})
    if method == "runtime.shutdown":
        assert "params" not in request
        if SESSION and saved()["mode"] == "unacknowledged-shutdown":
            sys.exit(0)
        response(request, None)
        sys.exit(0)
    static = {
        "connect": {"ok": True, "protocolVersion": 3, "version": "offline-fixture-c"},
        "registerExtensionLaunchProvider": None,
        "session.factory.listRuns": {"runs": []},
        "session.tools.initializeAndValidate": {"unexpected": True} if MODE == "invalid-initialize-ack" else {},
        "session.tools.getCurrentMetadata": metadata() if CONFIG else {},
        "session.agent.list": catalog.agents(CONFIG, SESSION, MODE) if CONFIG else {},
        "session.skills.list": catalog.skills(SESSION, MODE) if SESSION else {},
        "session.commands.list": catalog.commands(SESSION, MODE) if SESSION else {},
        "session.mcp.listTools": {"tools": [{"name": "get_step_context"}, {"name": "publish_result"}]},
        "session.extensions.list": {"extensions": [{
            "id": f"session:{SESSION.name}:{SESSION.name}" if SESSION else "",
            "name": SESSION.name if SESSION else "", "source": "session", "status": "running"
        }]},
    }
    assert method in static, f"unexpected SDK method: {method}"
    response(request, static[method])


trace({"argv": sys.argv[1:], "home": os.environ["HOME"], "copilotHome": str(HOME),
       **authentication.startup(MODE)})
try:
    while True:
        request = receive()
        dispatch(request)
except EOFError:
    pass
