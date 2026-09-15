"""Offline assertions for the SDK's registered filter and separate auth channels."""
import json
import os
from pathlib import Path
import subprocess
import sys

NAME = "COPILOT_GITHUB_TOKEN"
ALIASES = ("GITHUB_TOKEN", "GITHUB_COPILOT_GITHUB_TOKEN")
REPOSITORY_TOKEN = "offline-repository-token"
FORGE_TOKEN = None
SHELL_CREDENTIALS = None


def sandbox_auth():
    settings = json.loads(Path(os.environ["COPILOT_HOME"]).joinpath("settings.json").read_text())
    return settings["sandbox"]["auth"]


def credential(value):
    return {"offline-model-token": "original",
            "offline-rotated-model-token": "rotated"}.get(value, "unapproved")


def redacted(value):
    params = value.get("params")
    if not isinstance(params, dict) or "gitHubToken" not in params:
        return value
    return {**value, "modelCredential": credential(params["gitHubToken"]),
            "params": {**params, "gitHubToken": "[redacted session credential]"}}


def forge_credential(environment):
    assert environment.get("GH_TOKEN") == FORGE_TOKEN, "forge auth must match its separate grant"
    return "repository" if FORGE_TOKEN else None


def startup(mode):
    global FORGE_TOKEN
    FORGE_TOKEN = {"mixed-credentials": REPOSITORY_TOKEN,
                   "same-value-credentials": "offline-model-token"}.get(mode)
    args = sys.argv[1:]
    assert args[args.index("--auth-token-env") + 1] == NAME
    assert "--no-auto-login" in args
    assert "--secret-env-vars=" + NAME in args
    assert "--sandbox" in args and "--no-sandbox" not in args
    assert sandbox_auth() == {"git": False, "gh": False}
    assert not any(name in os.environ for name in ALIASES)
    token = os.environ[NAME]
    assert token and all(token not in argument for argument in args)
    if mode == "startup-auth-rejected":
        sys.stderr.write("offline startup authentication rejected: " + token + "\n")
        sys.exit(13)
    return {"startupCredential": credential(token), "ambientGitHubCredential": False,
            "forgeCredential": forge_credential(os.environ)}


def session(params):
    global SHELL_CREDENTIALS
    assert params["gitHubToken"] == os.environ[NAME], "cold session auth must match startup"
    assert params["shell"]["credentials"] == {"git": False, "gh": False}
    assert "builtin:lsp" in params["excludedTools"] and "builtin:lsp" not in params["availableTools"]
    SHELL_CREDENTIALS = params["shell"]["credentials"]


def descendant(kind, overlay):
    # C composes a filtered inherited map, then overlays; the overlay is not replacement.
    blocked = {"COPILOT_GITHUB_TOKEN", "GITHUB_TOKEN"}
    blocked.update(next(arg.split("=", 1)[1] for arg in sys.argv
                        if arg.startswith("--secret-env-vars=")).split(","))
    env = {key: value for key, value in os.environ.items() if key not in blocked}
    env.update(overlay)
    if kind == "shell" and any(SHELL_CREDENTIALS[channel] or sandbox_auth()[channel]
                               for channel in ("git", "gh")):
        env["GH_TOKEN"] = os.environ[NAME]
    command = [sys.executable, "-c", "import json,os; print(json.dumps(dict(os.environ)))"]
    if kind == "shell":
        command = [sys.executable, str(Path(__file__).with_name("gh.py"))]
    child = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
    inherited = json.loads(child.stdout)
    assert inherited["HOME"] == os.environ["HOME"], "overlay must not fake replacement"
    assert not any(name in inherited for name in (NAME, *ALIASES))
    # Equal bytes in the separately authorized forge channel are not a new model carrier.
    other_channels = {key: value for key, value in inherited.items() if key != "GH_TOKEN"}
    assert os.environ[NAME] not in json.dumps(other_channels)
    return {"authDescendant": kind, "modelCarrierPresent": False, "inheritedHome": True,
            "forgeCredential": forge_credential(inherited),
            "forgeCredentialMatchesModel": FORGE_TOKEN == os.environ[NAME],
            "probeCommand": "gh" if kind == "shell" else None}
