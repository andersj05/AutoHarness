use autoharness_domain::{
    AttemptId, CommandPayload, ContextTurnId, EventPayload, SessionId, Sha256Digest,
};

#[test]
fn context_turn_session_binding_shapes_are_stable() {
    let command = CommandPayload::BindContextTurn {
        session_id: SessionId::new("session-1").expect("valid session ID"),
        attempt_id: AttemptId::new("attempt-1").expect("valid attempt ID"),
        run_turn: 2,
        context_turn_id: ContextTurnId::new("context-turn-2").expect("valid context turn ID"),
        manifest_hash: digest('a'),
    };
    let event = EventPayload::ContextTurnBound {
        attempt_id: AttemptId::new("attempt-1").expect("valid attempt ID"),
        run_turn: 2,
        context_turn_id: ContextTurnId::new("context-turn-2").expect("valid context turn ID"),
        manifest_hash: digest('a'),
    };

    assert_eq!(
        serde_json::to_value(command).expect("serialize binding command"),
        serde_json::json!({
            "kind": "bind_context_turn",
            "payload": {
                "session_id": "session-1",
                "attempt_id": "attempt-1",
                "run_turn": 2,
                "context_turn_id": "context-turn-2",
                "manifest_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(event).expect("serialize binding event"),
        serde_json::json!({
            "kind": "context_turn_bound",
            "payload": {
                "attempt_id": "attempt-1",
                "run_turn": 2,
                "context_turn_id": "context-turn-2",
                "manifest_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        })
    );
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::new(character.to_string().repeat(64)).expect("valid digest")
}
