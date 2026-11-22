// Compile the actual BiDi reader with a test-only connector. The transport is
// a pair of in-memory WebSockets, so the test never needs a listening port.
extern crate self as tokio_tungstenite;
extern crate tokio_tungstenite as real_tungstenite;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::DuplexStream;
use tokio_tungstenite::tungstenite::protocol::Role;

pub use real_tungstenite::tungstenite;

pub mod error {
    pub use selenium_core::error::*;
}

#[path = "../src/bidi.rs"]
mod bidi;

static CLIENT_STREAMS: OnceLock<Mutex<HashMap<String, DuplexStream>>> = OnceLock::new();

fn client_streams() -> &'static Mutex<HashMap<String, DuplexStream>> {
    CLIENT_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn connect_async(
    url: &str,
) -> std::io::Result<(real_tungstenite::WebSocketStream<DuplexStream>, ())> {
    let stream = client_streams()
        .lock()
        .unwrap()
        .remove(url)
        .expect("test must install an in-memory client stream");
    Ok((
        real_tungstenite::WebSocketStream::from_raw_socket(stream, Role::Client, None).await,
        (),
    ))
}

#[tokio::test]
async fn event_between_out_of_order_responses_preserves_command_correlation() {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let url = "ws://in-memory/correlation";
    client_streams().lock().unwrap().insert(url.into(), client);
    let conn = bidi::Connection::connect(url).await.unwrap();
    let mut events = conn.subscribe_events();
    let mut peer =
        real_tungstenite::WebSocketStream::from_raw_socket(server, Role::Server, None).await;

    let exchange = async {
        let server = async {
            let first: Value =
                serde_json::from_str(peer.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            let second: Value =
                serde_json::from_str(peer.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_ne!(first["id"], second["id"]);

            for command in [&second, &first] {
                if command == &first {
                    peer.send(tungstenite::Message::text(
                        json!({
                            "type": "event",
                            "method": "network.beforeRequestSent",
                            "params": {
                                "context": "page-1",
                                "request": {"request": "request-1", "url": "https://example.test/", "method": "GET"},
                                "redirectCount": 0
                            }
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                }
                peer.send(tungstenite::Message::text(
                    json!({
                        "type": "success",
                        "id": command["id"],
                        "result": {"echo": command["method"]}
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            }
        };
        let (one, two, ()) = tokio::join!(
            conn.send("test.first", json!({"value": 1})),
            conn.send("test.second", json!({"value": 2})),
            server,
        );
        assert_eq!(one.unwrap(), json!({"echo": "test.first"}));
        assert_eq!(two.unwrap(), json!({"echo": "test.second"}));
        let event = events.recv().await.unwrap();
        assert_eq!(event.method, "network.beforeRequestSent");
        assert_eq!(
            event.params,
            json!({
                "context": "page-1",
                "request": {"request": "request-1", "url": "https://example.test/", "method": "GET"},
                "redirectCount": 0
            })
        );
    };

    tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .expect("in-memory exchange did not complete");
}

#[tokio::test]
async fn slow_event_receiver_reports_overflow_and_retains_newest_events() {
    let (client, server) = tokio::io::duplex(128 * 1024);
    let url = "ws://in-memory/overflow";
    client_streams().lock().unwrap().insert(url.into(), client);
    let conn = bidi::Connection::connect(url).await.unwrap();
    let mut events = conn.subscribe_events();
    let mut peer =
        real_tungstenite::WebSocketStream::from_raw_socket(server, Role::Server, None).await;

    let exchange = async {
        let server = async {
            for sequence in 0..257 {
                peer.send(tungstenite::Message::text(
                    json!({
                        "type": "event",
                        "method": "network.beforeRequestSent",
                        "params": {"sequence": sequence}
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            }
            // The response proves the reader processed every preceding event.
            let command: Value =
                serde_json::from_str(peer.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            peer.send(tungstenite::Message::text(
                json!({"type": "success", "id": command["id"], "result": {}}).to_string(),
            ))
            .await
            .unwrap();
        };
        let (result, ()) = tokio::join!(conn.send("test.barrier", json!({})), server);
        assert_eq!(result.unwrap(), json!({}));
        assert!(matches!(
            events.recv().await,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(1))
        ));
        let retained = events.recv().await.unwrap();
        assert_eq!(retained.method, "network.beforeRequestSent");
        assert_eq!(retained.params, json!({"sequence": 1}));
    };

    tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .expect("in-memory overflow exchange did not complete");
}

#[tokio::test]
async fn peer_close_signals_connection_closed_subscriber() {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let url = "ws://in-memory/close";
    client_streams().lock().unwrap().insert(url.into(), client);
    let conn = bidi::Connection::connect(url).await.unwrap();
    let mut closed = conn.subscribe_closed();
    let mut peer =
        real_tungstenite::WebSocketStream::from_raw_socket(server, Role::Server, None).await;

    assert!(!*closed.borrow());
    peer.send(tungstenite::Message::Close(None)).await.unwrap();
    drop(peer);

    tokio::time::timeout(Duration::from_secs(5), closed.changed())
        .await
        .expect("connection close signal did not arrive")
        .expect("close signal sender dropped without an update");
    assert!(*closed.borrow_and_update());
}
