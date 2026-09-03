//! Invalid-call repair and human tool-permission scenarios through the real terminal.

mod pty_support;

use pty_support::{PtySession, RouterFixture, ScenarioEnvironment, ctrl_c};

const CTRL_P: [u8; 1] = [0x10];
const CTRL_S: [u8; 1] = [0x13];

fn select_fixture_model(session: &mut PtySession) {
    session.wait_for(
        |screen| screen.contents().contains("PTY Router"),
        "live fixture catalog should reach the terminal",
    );
    session.send_bytes(&CTRL_P);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Models") && text.contains("PTY Router")
        },
        "Ctrl+P should open the live model picker",
    );
    session.send_bytes(b"\r");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY Router") && text.contains("Ask Agent") && !text.contains("Models")
        },
        "Enter should select the fixture model and return to the composer",
    );
}

fn submit_prompt(session: &mut PtySession, prompt: &str) {
    session.type_text(prompt);
    session.send_bytes(&CTRL_S);
}

fn tool_call(call_id: &str, tool: &str, arguments: &str) -> String {
    let data = serde_json::json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": call_id,
                    "type": "function",
                    "function": {"name": tool, "arguments": arguments}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });
    format!("data: {data}\n\ndata: [DONE]\n\n")
}

fn completion(text: &str) -> String {
    let data = serde_json::json!({
        "choices": [{"delta": {"content": text}, "finish_reason": "stop"}]
    });
    format!("data: {data}\n\ndata: [DONE]\n\n")
}

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn invalid_tool_call_is_denied_and_repaired_in_the_same_terminal_attempt() {
    let fixture = RouterFixture::start(vec![
        tool_call("call-invalid", "web_search", r#"{"query":"news"}"#),
        completion("recovered after invalid tool call"),
    ]);
    let mut environment = ScenarioEnvironment::prepare();
    fixture.configure(&mut environment);
    let mut session = PtySession::start(&environment, 30, 100);
    select_fixture_model(&mut session);

    submit_prompt(&mut session, "request an unavailable search tool");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("web_search")
                && text.contains("denied")
                && text.contains("recovered after invalid tool call")
                && text.contains("AutoHarness")
                && text.contains("complete")
        },
        "invalid tool proposal should be force-denied and repaired without human authority",
    );
    assert!(
        !environment.data_dir().join("news").exists(),
        "invalid tool calls must never gain an external capability"
    );

    session.send_bytes(&ctrl_c());
    assert_eq!(
        session.wait_for_exit(),
        0,
        "invalid-call scenario exits cleanly"
    );
    let requests = fixture.finish();
    assert_eq!(
        requests,
        vec![
            "GET /v1/models?limit=1000",
            "POST /v1/chat/completions",
            "POST /v1/chat/completions",
        ]
    );
}

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn permission_deny_and_allow_both_settle_durably() {
    let fixture = RouterFixture::start(vec![
        tool_call(
            "call-deny",
            "fs_write",
            r#"{"path":"denied.txt","content":"must not exist"}"#,
        ),
        completion("denied call handled"),
        tool_call(
            "call-allow",
            "fs_write",
            r#"{"path":"allowed.txt","content":"durable permission result"}"#,
        ),
        completion("allowed call handled"),
    ]);
    let mut environment = ScenarioEnvironment::prepare();
    fixture.configure(&mut environment);
    let mut session = PtySession::start(&environment, 32, 110);
    select_fixture_model(&mut session);

    submit_prompt(&mut session, "write the denied fixture");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Tool permission")
                && text.contains("fs_write")
                && text.contains("denied.txt")
        },
        "a real pending filesystem call should open the scoped permission overlay",
    );
    session.send_bytes(b"n");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("fs_write")
                && text.contains("denied")
                && text.contains("denied call handled")
        },
        "N should durably deny the exact call and continue the provider turn",
    );
    assert!(
        !environment.data_dir().join("denied.txt").exists(),
        "denied filesystem effect must not run"
    );

    submit_prompt(&mut session, "write the allowed fixture");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Tool permission")
                && text.contains("fs_write")
                && text.contains("allowed.txt")
        },
        "the second real call should require an independent permission answer",
    );
    session.send_bytes(b"y");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("fs_write")
                && text.contains("completed")
                && text.contains("allowed call handled")
        },
        "Y should run the exact allowed call and continue after durable settlement",
    );
    assert_eq!(
        std::fs::read_to_string(environment.data_dir().join("allowed.txt"))
            .expect("allowed filesystem result"),
        "durable permission result"
    );

    session.send_bytes(&ctrl_c());
    assert_eq!(
        session.wait_for_exit(),
        0,
        "permission scenario exits cleanly"
    );
    let requests = fixture.finish();
    assert_eq!(
        requests.len(),
        5,
        "catalog plus four provider turns expected"
    );

    environment.remove("AUTOHARNESS_ROUTER_API_KEY");
    let mut replay = PtySession::start(&environment, 32, 110);
    replay.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("denied call handled")
                && text.contains("allowed call handled")
                && text.contains("fs_write")
        },
        "permission decisions and tool settlements should replay without a credential editor",
    );
    replay.send_bytes(&ctrl_c());
    assert_eq!(replay.wait_for_exit(), 0, "permission replay exits cleanly");
}
