use std::sync::Arc;

use autoharness_client::runtime::{
    APP_NOTICE_CAPACITY, ApiCredential, CatalogProjection, INTENT_CAPACITY, RequestId,
    SessionProjection, SessionsProjection, UiIntent, UiNotice, bounded_ports,
};
use tokio::sync::mpsc::error::TrySendError;

#[test]
fn runtime_channels_preserve_backpressure_and_latest_projection() {
    let (mut client, mut host) = bounded_ports(
        Arc::new(SessionProjection::empty()),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::CredentialRequired),
    );
    for sequence in 0..INTENT_CAPACITY {
        client
            .intents
            .try_send(UiIntent::CreateSession {
                request_id: RequestId::new(sequence as u64),
            })
            .expect("bounded admission");
    }
    assert!(matches!(
        client.intents.try_send(UiIntent::CreateSession {
            request_id: RequestId::new(999),
        }),
        Err(TrySendError::Full(_))
    ));
    for sequence in 0..INTENT_CAPACITY {
        assert_eq!(
            host.intents
                .try_recv()
                .expect("ordered intent")
                .request_id()
                .get(),
            sequence as u64
        );
    }
    for sequence in 0..APP_NOTICE_CAPACITY {
        host.notices
            .try_send(UiNotice::IntentCommitted {
                request_id: RequestId::new(sequence as u64),
            })
            .expect("bounded settlement");
    }
    assert!(matches!(
        host.notices.try_send(UiNotice::IntentCommitted {
            request_id: RequestId::new(999),
        }),
        Err(TrySendError::Full(_))
    ));
    assert_eq!(
        client.notices.try_recv().expect("first settlement"),
        UiNotice::IntentCommitted {
            request_id: RequestId::new(0)
        }
    );
    for revision in 1..=100 {
        let mut projection = SessionProjection::empty();
        projection.revision = revision;
        host.sessions.send_replace(Arc::new(projection));
    }
    assert_eq!(client.sessions.borrow_and_update().revision, 100);
    drop(host);
    assert!(matches!(
        client.intents.try_send(UiIntent::CreateSession {
            request_id: RequestId::new(1000),
        }),
        Err(TrySendError::Closed(_))
    ));
}

#[test]
fn runtime_secret_ingress_keeps_its_bound_and_redacted_diagnostics() {
    const SENTINEL: &str = "runtime-credential-sentinel";
    let credential = ApiCredential::new(SENTINEL.to_owned()).expect("valid credential");
    let intent = UiIntent::ConfigureCredential {
        request_id: RequestId::new(7),
        credential,
    };
    assert!(!format!("{intent:?}").contains(SENTINEL));
    assert!(ApiCredential::new(String::new()).is_err());
    assert!(ApiCredential::new("a".repeat(4097)).is_err());
    assert!(ApiCredential::new("key\ncontrol".to_owned()).is_err());
    let UiIntent::ConfigureCredential { credential, .. } = intent else {
        unreachable!()
    };
    assert_eq!(credential.into_string(), SENTINEL);
}
