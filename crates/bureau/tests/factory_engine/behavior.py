"""Offline protocol failure/control scenarios; never a provider or model."""
import json
import socket
import subprocess
import time
from pathlib import Path


def reject_admission(request, mode, send):
    if not mode.startswith("rejection-"):
        return False
    if mode == "rejection-operator-pause":
        Path.cwd().parent.joinpath("PAUSE").write_text("operator pause\n")
    send({"jsonrpc": "2.0", "id": request["id"], "error": {
        "code": -32602, "message": "synthetic definite admission rejection",
        "data": {"code": "factory_not_found"}}})
    return True


def before_shutdown(mode):
    if mode == "rejection-shutdown-pause":
        Path.cwd().parent.joinpath("PAUSE").write_text("operator pause\n")
    if mode == "rejection-unacknowledged-shutdown":
        raise SystemExit(0)


def permissions(request, session, notify, receive, response, malformed=False, kind="shell"):
    data = {"requestId": "permission-1", "permissionRequest": {"kind": kind,
            "extensionName": "unapproved", "environmentVariables": ["COPILOT_GITHUB_TOKEN"]},
            "resolvedByHook": True}
    notify("permission.requested", data)
    data["resolvedByHook"] = False
    for _ in range(2):
        notify("permission.requested", data)
        reply = receive()
        assert reply["method"] == "session.permissions.handlePendingPermissionRequest"
        assert reply["id"] != request["id"]
        assert reply["params"] == {
            "sessionId": session.name, "requestId": "permission-1",
            "result": {"kind": "reject", "feedback": "Denied by Bureau policy."}}
        response(reply, {"success": "invalid" if malformed else False})
        if malformed:
            break


def failure(state):
    if state["mode"] == "accounting-incomplete":
        return {"type": "factory_accounting_incomplete",
                "runId": "known-offline-run", "drainedNanoAiu": 1_000_000_000}
    if state["mode"] == "halted":
        return {"type": "factory_limit_reached", "kind": "maxTotalSubagents",
                "value": 3, "runId": "known-offline-run"}
    return None


def before_observe(request, state, session, config, notify, receive, response):
    mode = state["mode"]
    if mode in ("permissions", "bad-permission-ack") and config["requestExtensions"]:
        permissions(request, session, notify, receive, response, mode == "bad-permission-ack")
    if mode == "credential-permission" and config["requestExtensions"]:
        permissions(request, session, notify, receive, response, kind="extension-env-access")
    if mode == "hard-timeout" and config["requestExtensions"]:
        signal = Path.cwd().parents[2] / "admission-socket"
        if signal.exists():
            with socket.socket(socket.AF_UNIX) as notification:
                notification.connect("\0" + signal.read_text())
        child = str(session / "orphan-survived")
        process = subprocess.Popen(["/usr/bin/python3", str(Path(__file__).with_name("lifetime.py")),
                                    child, str(Path.cwd().parents[2] / "liveness-socket")],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        (session / "fixture-child.pid").write_text(str(process.pid))
        time.sleep(20)
    if mode == "accounting-incomplete":
        state["status"] = "error"
    if mode == "halted" and state["attempt"] == 1:
        state["status"] = "halted"
    if mode == "unrequested-cancel":
        state["status"] = "paused"


def restore_effect(mode, attempt):
    if mode in ("pause", "pause-hang", "resume-wrong-id") and attempt == 1:
        path = Path.cwd() / "factory-uncommitted.txt"
        if not path.exists():
            path.write_text("preserved factory effect\n")


def resumed_effect(mode):
    if mode in ("pause", "pause-hang", "resume-wrong-id"):
        path = Path.cwd() / "factory-uncommitted.txt"
        assert path.read_text() == "preserved factory effect\n", "factory effects were reset"
        path.unlink()
