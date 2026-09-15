use serde_json::json;

use super::super::agent;
use super::support::{Fixture, plan, role, settings, write};

#[test]
fn direct_prompt_is_literal_and_preserves_metadata_and_body_bytes() {
    let fixture = Fixture::new();
    let role = role(".github/agents/worker.agent.md");
    let mut plan = plan(&role);
    let prompt = "\r\nLiteral body: { untouched }\r\n  keep indentation\r\n";
    plan.direct_agents.insert(role.name.clone(), format!(
        "---\r\nname: worker\r\ndescription: Reviewed\r\ntools: view, grep\r\nmodel: reviewed-model\r\nmodelPolicy: fallback\r\nreasoningEffort: high\r\n---\r\n{prompt}"
    ).into_bytes());
    write(&fixture.worktree.join(&role.agent), b"Mutable replacement.");
    let agent = fixture
        .prepare(&plan, &role)
        .expect("literal agent")
        .custom_agent
        .expect("agent");
    assert_eq!(
        serde_json::to_value(agent).expect("native definition"),
        json!({"name": "worker", "prompt": prompt, "description": "Reviewed",
            "tools": ["view", "grep"], "model": "reviewed-model", "modelPolicy": "fallback", "reasoningEffort": "high"})
    );
}

#[test]
fn direct_agent_rejects_executable_or_unsupported_frontmatter() {
    let cases = [
        "mcpServers:\n  other:\n    command: unsafe-command\n",
        "mcp-servers:\n  bureau-io:\n    command: bureau\n",
        "hooks:\n  sessionStart: unsafe-command\n",
        "executable: unapproved\n",
        "tools: [view, 123]\n",
        "name: different\n",
    ];
    for metadata in cases {
        let text = format!("---\n{metadata}---\nBody.\n");
        assert!(
            agent::parse(text.as_bytes(), "worker").is_err(),
            "{metadata}"
        );
    }
}

#[test]
fn agent_body_without_frontmatter_is_not_synthesized() {
    let prompt = b"\nDo exactly this.\n\n";
    assert_eq!(
        agent::parse(prompt, "worker")
            .expect("literal")
            .prompt
            .as_bytes(),
        prompt
    );
}

#[test]
fn a_direct_agent_cannot_hydrate_skills_from_unpinned_ambient_sources() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    let mut plan = plan(&role);
    plan.direct_agents.insert(
        role.name.clone(),
        b"---\nname: worker\nskills: [ambient]\n---\nBody.\n".to_vec(),
    );
    assert!(
        fixture
            .prepare(&plan, &role)
            .expect_err("unpinned skill")
            .contains("not provided")
    );
}

#[test]
fn direct_agent_preserves_an_explicit_pinned_plugin_skill_selection() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    let mut plan = plan(&role);
    fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    plan.direct_agents.insert(
        role.name.clone(),
        b"---\nname: worker\nskills: ['review:review']\n---\nBody.\n".to_vec(),
    );
    let pins = fixture.prepare(&plan, &role).expect("pinned skill");
    assert_eq!(
        pins.custom_agent.expect("direct agent").skills,
        Some(vec!["review:review".to_owned()])
    );
}

#[test]
fn unselected_plugin_agent_inline_mcp_is_a_preinitialization_error() {
    let fixture = Fixture::new();
    let role = role("/review:worker");
    let directory = fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    write(
        &directory.join("agents/other.agent.md"),
        b"---\nname: other\nmcpServers:\n  other:\n    command: unapproved\n---\nBody.\n",
    );
    assert!(
        fixture
            .prepare(&plan(&role), &role)
            .expect_err("inline MCP")
            .contains("inline MCP")
    );
}

#[test]
fn plugin_hooks_and_non_bureau_mcp_are_refused() {
    let cases = [
        ("hooks/hooks.json", json!({"hooks": {"sessionStart": []}})),
        (
            ".mcp.json",
            json!({"mcpServers": {"other": {"command": "unapproved", "args": []}}}),
        ),
        (
            ".mcp.json",
            json!({"mcpServers": {"bureau-io": {"command": "bureau", "args": ["mcp", "serve"], "env": {"EXTRA": "value"}}}}),
        ),
    ];
    for (path, value) in cases {
        let fixture = Fixture::new();
        let role = role("/review:worker");
        let directory = fixture.plugin("review");
        fixture.set_settings(&settings(json!({"review@local": true})));
        write(
            &directory.join(path),
            serde_json::to_vec(&value).expect("JSON"),
        );
        assert!(fixture.prepare(&plan(&role), &role).is_err());
    }
}

#[test]
fn exact_bureau_io_declaration_records_the_required_parent_binding() {
    let fixture = Fixture::new();
    let role = role("/review:worker");
    let directory = fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    write(&directory.join(".mcp.json"),
        br#"{"mcpServers":{"bureau-io":{"type":"stdio","command":"bureau","args":["mcp","serve"]}}}"#);
    let pins = fixture.prepare(&plan(&role), &role).expect("known MCP");
    assert_eq!(
        pins.expected_catalog.bureau_io_plugins,
        ["review".to_owned()].into()
    );
}

#[test]
fn repository_hooks_are_not_silently_discarded() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    write(&fixture.worktree.join(".github/hooks/session.json"), b"{}");
    assert!(
        fixture
            .prepare(&plan(&role), &role)
            .expect_err("repo hooks")
            .contains("hooks/MCP")
    );
}
