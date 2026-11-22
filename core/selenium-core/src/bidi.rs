//! Minimal WebDriver BiDi connection: id-correlated commands over a WebSocket.

use crate::error::{Error, ErrorKind, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

const EVENT_BUFFER_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub(crate) struct BiDiEvent {
    pub method: String,
    pub params: Value,
}

pub struct Connection {
    next_id: AtomicU64,
    pending: Pending,
    tx: mpsc::UnboundedSender<String>,
    events: broadcast::Sender<BiDiEvent>,
    closed: watch::Receiver<bool>,
}

impl Connection {
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| Error::protocol(format!("websocket connect: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::default();
        let (events, _) = broadcast::channel(EVENT_BUFFER_CAPACITY);
        let (closed_tx, closed) = watch::channel(false);

        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if sink.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
        });

        let p = pending.clone();
        let e = events.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let Some(id) = v["id"].as_u64() else {
                    if let Some(method) = v["method"].as_str() {
                        let _ = e.send(BiDiEvent {
                            method: method.to_owned(),
                            params: v["params"].clone(),
                        });
                    }
                    continue;
                };
                let Some(waiter) = p.lock().unwrap().remove(&id) else {
                    continue;
                };
                let _ = waiter.send(if v["type"] == "success" {
                    Ok(v["result"].clone())
                } else {
                    Err(map_error(&v))
                });
            }
            let _ = closed_tx.send(true);
            // Connection closed: fail everything still waiting.
            for (_, w) in p.lock().unwrap().drain() {
                let _ = w.send(Err(Error::protocol("connection closed")));
            }
        });

        Ok(Self {
            next_id: AtomicU64::new(1),
            pending,
            tx,
            events,
            closed,
        })
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<BiDiEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_closed(&self) -> watch::Receiver<bool> {
        self.closed.clone()
    }

    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (otx, orx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, otx);
        let msg = json!({ "id": id, "method": method, "params": params }).to_string();
        if self.tx.send(msg).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(Error::protocol("connection closed"));
        }
        match tokio::time::timeout(Duration::from_secs(120), orx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(Error::protocol("connection closed")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(Error::new(
                    ErrorKind::Timeout,
                    format!("{method} timed out"),
                ))
            }
        }
    }
}

fn map_error(v: &Value) -> Error {
    let code = v["error"].as_str().unwrap_or("unknown error");
    let msg = v["message"].as_str().unwrap_or("");
    let kind = match code {
        "no such frame" => ErrorKind::NoSuchContext,
        "invalid argument" => ErrorKind::InvalidArgument,
        "timeout" => ErrorKind::Timeout,
        _ => ErrorKind::Protocol,
    };
    Error::new(kind, format!("{code}: {msg}"))
}
