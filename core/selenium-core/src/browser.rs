use crate::bidi::{BiDiEvent, Connection};
use crate::driver::{self, DriverProcess};
use crate::error::{Error, ErrorKind, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};

#[derive(Debug, Clone, Default)]
pub struct ChromeOptions {
    pub headless: bool,
    pub binary_path: Option<String>,
    pub driver_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserStatus {
    pub ready: bool,
    pub message: String,
}

struct Inner {
    conn: Connection,
    network_subscribed: Mutex<bool>,
    http: reqwest::Client,
    session_url: String,
    // Declared last so the driver process is killed after the session is deleted.
    driver: DriverProcess,
}

/// A running browser: owns the driver process and the BiDi session.
#[derive(Clone)]
pub struct Browser(Arc<Inner>);

#[derive(Clone)]
pub struct UserContext {
    browser: Browser,
    pub id: String,
}

#[derive(Clone)]
pub struct Page {
    browser: Browser,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct NetworkBeforeRequest {
    request_id: String,
    url: String,
    method: String,
    redirect_count: i32,
}

pub struct NetworkEventSubscription {
    events: Mutex<broadcast::Receiver<BiDiEvent>>,
    closed: tokio::sync::watch::Receiver<bool>,
}

impl Browser {
    pub async fn launch_chrome(options: ChromeOptions) -> Result<Browser> {
        let driver = driver::spawn(
            options.driver_path.as_deref().map(Path::new),
            options.binary_path.as_deref().map(Path::new),
        )
        .await?;
        let http = reqwest::Client::new();

        let mut args: Vec<&str> = vec![];
        if options.headless {
            args.push("--headless=new");
        }
        let mut chrome_opts = json!({ "args": args });
        if let Some(b) = &options.binary_path {
            chrome_opts["binary"] = json!(b);
        }
        let body = json!({ "capabilities": { "alwaysMatch": {
            "browserName": "chrome",
            "webSocketUrl": true,
            "goog:chromeOptions": chrome_opts,
        }}});

        let resp: Value = http
            .post(format!("{}/session", driver.base_url()))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::launch(format!("create session: {e}")))?
            .json()
            .await
            .map_err(|e| Error::launch(format!("create session: {e}")))?;
        let value = &resp["value"];
        let session_id = value["sessionId"].as_str().ok_or_else(|| {
            Error::launch(format!(
                "create session failed: {}",
                value["message"].as_str().unwrap_or("unknown")
            ))
        })?;
        let ws_url = value["capabilities"]["webSocketUrl"]
            .as_str()
            .ok_or_else(|| Error::launch("browser did not provide a BiDi webSocketUrl"))?;

        let conn = Connection::connect(ws_url).await?;
        let session_url = format!("{}/session/{}", driver.base_url(), session_id);
        Ok(Browser(Arc::new(Inner {
            conn,
            network_subscribed: Mutex::new(false),
            http,
            session_url,
            driver,
        })))
    }

    pub async fn get_status(&self) -> Result<BrowserStatus> {
        get_status(&self.0.conn).await
    }

    /// Receives non-blocking notifications for network requests; requests are not intercepted.
    pub async fn subscribe_network_before_request(&self) -> Result<NetworkEventSubscription> {
        subscribe_network_before_request(&self.0.conn, &self.0.network_subscribed).await
    }

    pub async fn default_user_context(&self) -> Result<UserContext> {
        Ok(UserContext {
            browser: self.clone(),
            id: "default".into(),
        })
    }

    pub async fn user_contexts(&self) -> Result<Vec<UserContext>> {
        let r = self
            .0
            .conn
            .send("browser.getUserContexts", json!({}))
            .await?;
        Ok(r["userContexts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["userContext"].as_str())
            .map(|id| UserContext {
                browser: self.clone(),
                id: id.to_string(),
            })
            .collect())
    }

    pub async fn create_user_context(&self) -> Result<UserContext> {
        let r = self
            .0
            .conn
            .send("browser.createUserContext", json!({}))
            .await?;
        let id = r["userContext"]
            .as_str()
            .ok_or_else(|| Error::protocol("missing userContext"))?;
        Ok(UserContext {
            browser: self.clone(),
            id: id.to_string(),
        })
    }

    /// The first top-level page of the default user context.
    pub async fn default_page(&self) -> Result<Page> {
        self.default_user_context()
            .await?
            .pages()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::protocol("no pages in default user context"))
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.default_page().await?.navigate(url).await
    }

    /// Ends the session and stops the driver, regardless of outstanding page/context handles.
    pub async fn close(&self) -> Result<()> {
        let _ = self.0.http.delete(&self.0.session_url).send().await;
        self.0.driver.kill();
        Ok(())
    }

    pub fn driver_port(&self) -> u16 {
        self.0.driver.port
    }
}

impl UserContext {
    pub async fn pages(&self) -> Result<Vec<Page>> {
        let r = self
            .browser
            .0
            .conn
            .send("browsingContext.getTree", json!({}))
            .await?;
        Ok(r["contexts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| {
                c["userContext"].as_str() == Some(self.id.as_str()) && c["parent"].is_null()
            })
            .filter_map(|c| c["context"].as_str())
            .map(|id| Page {
                browser: self.browser.clone(),
                id: id.to_string(),
            })
            .collect())
    }

    pub async fn new_page(&self) -> Result<Page> {
        let r = self
            .browser
            .0
            .conn
            .send(
                "browsingContext.create",
                json!({ "type": "tab", "userContext": self.id }),
            )
            .await?;
        let id = r["context"]
            .as_str()
            .ok_or_else(|| Error::protocol("missing context"))?;
        Ok(Page {
            browser: self.browser.clone(),
            id: id.to_string(),
        })
    }

    pub async fn remove(&self) -> Result<()> {
        self.browser
            .0
            .conn
            .send(
                "browser.removeUserContext",
                json!({ "userContext": self.id }),
            )
            .await
            .map(|_| ())
    }
}

impl Page {
    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.browser
            .0
            .conn
            .send(
                "browsingContext.navigate",
                json!({ "context": self.id, "url": normalize_url(url), "wait": "complete" }),
            )
            .await
            .map(|_| ())
    }

    pub async fn close(&self) -> Result<()> {
        self.browser
            .0
            .conn
            .send("browsingContext.close", json!({ "context": self.id }))
            .await
            .map(|_| ())
    }
}

impl NetworkEventSubscription {
    /// Returns the next request event, or a timeout error after `timeout_ms`.
    pub async fn next(&self, timeout_ms: i32) -> Result<NetworkBeforeRequest> {
        if timeout_ms < 0 {
            return Err(Error::new(
                ErrorKind::InvalidArgument,
                "timeout must not be negative",
            ));
        }

        let mut events = self.events.lock().await;
        let mut closed = self.closed.clone();
        let timeout = Duration::from_millis(timeout_ms as u64);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let event = if timeout_ms == 0 || *closed.borrow() {
                match try_receive_event(&mut events, *closed.borrow())? {
                    Some(event) => event,
                    None => return Err(timeout_error()),
                }
            } else {
                match tokio::time::timeout_at(deadline, async {
                    tokio::select! {
                        biased;
                        event = events.recv() => event.map_err(|error| match error {
                            broadcast::error::RecvError::Lagged(skipped) => event_overflow(skipped),
                            broadcast::error::RecvError::Closed => connection_closed(),
                        }),
                        changed = closed.changed() => {
                            let _ = changed;
                            Err(connection_closed())
                        }
                    }
                })
                .await
                {
                    Ok(Ok(event)) => event,
                    Ok(Err(error)) => return Err(error),
                    Err(_) => return Err(timeout_error()),
                }
            };

            if event.method != "network.beforeRequestSent" {
                continue;
            }
            return parse_network_before_request(event);
        }
    }
}

fn try_receive_event(
    events: &mut broadcast::Receiver<BiDiEvent>,
    closed: bool,
) -> Result<Option<BiDiEvent>> {
    match events.try_recv() {
        Ok(event) => Ok(Some(event)),
        Err(broadcast::error::TryRecvError::Empty) if closed => Err(connection_closed()),
        Err(broadcast::error::TryRecvError::Empty) => Ok(None),
        Err(broadcast::error::TryRecvError::Lagged(skipped)) => Err(event_overflow(skipped)),
        Err(broadcast::error::TryRecvError::Closed) => Err(connection_closed()),
    }
}

async fn subscribe_network_before_request(
    conn: &Connection,
    subscribed: &Mutex<bool>,
) -> Result<NetworkEventSubscription> {
    let events = conn.subscribe_events();
    let mut subscribed = subscribed.lock().await;
    if !*subscribed {
        conn.send(
            "session.subscribe",
            json!({ "events": ["network.beforeRequestSent"] }),
        )
        .await?;
        *subscribed = true;
    }
    Ok(NetworkEventSubscription {
        events: Mutex::new(events),
        closed: conn.subscribe_closed(),
    })
}

impl NetworkBeforeRequest {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn redirect_count(&self) -> i32 {
        self.redirect_count
    }
}

fn parse_network_before_request(event: BiDiEvent) -> Result<NetworkBeforeRequest> {
    let request = event
        .params
        .get("request")
        .ok_or_else(|| Error::protocol("missing request in network.beforeRequestSent"))?;
    let required_string = |value: &Value, field: &str| {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::protocol(format!("missing {field} in network.beforeRequestSent")))
    };
    let redirect_count = event.params["redirectCount"]
        .as_u64()
        .and_then(|count| i32::try_from(count).ok())
        .ok_or_else(|| Error::protocol("invalid redirectCount in network.beforeRequestSent"))?;

    Ok(NetworkBeforeRequest {
        request_id: required_string(&request["request"], "request.request")?,
        url: required_string(&request["url"], "request.url")?,
        method: required_string(&request["method"], "request.method")?,
        redirect_count,
    })
}

fn timeout_error() -> Error {
    Error::new(
        ErrorKind::Timeout,
        "timed out waiting for network.beforeRequestSent",
    )
}

fn event_overflow(skipped: u64) -> Error {
    Error::new(
        ErrorKind::EventOverflow,
        format!("network event queue overflowed; {skipped} event(s) were lost"),
    )
}

fn connection_closed() -> Error {
    Error::protocol("BiDi event connection closed")
}

fn normalize_url(url: &str) -> String {
    if url.contains("://") || url.starts_with("about:") || url.starts_with("data:") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

async fn get_status(conn: &Connection) -> Result<BrowserStatus> {
    let response = conn.send("session.status", json!({})).await?;
    parse_browser_status(&response)
}

fn parse_browser_status(response: &Value) -> Result<BrowserStatus> {
    let ready = response["ready"]
        .as_bool()
        .ok_or_else(|| Error::protocol("missing ready in session.status result"))?;
    let message = response["message"]
        .as_str()
        .ok_or_else(|| Error::protocol("missing message in session.status result"))?;
    Ok(BrowserStatus {
        ready,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn browser_status_parses_readiness_and_message() {
        let status = parse_browser_status(&json!({
            "ready": true,
            "message": "Browser ready"
        }))
        .unwrap();
        assert_eq!(
            status,
            BrowserStatus {
                ready: true,
                message: "Browser ready".into(),
            }
        );
    }

    #[test]
    fn browser_status_rejects_missing_fields() {
        assert_eq!(
            parse_browser_status(&json!({"ready": true}))
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
        assert_eq!(
            parse_browser_status(&json!({"message": "starting"}))
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
    }

    #[tokio::test]
    async fn browser_status_uses_bidi_session_status_command() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let request = socket.next().await.unwrap().unwrap();
            let Message::Text(request) = request else {
                panic!("expected BiDi command text");
            };
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "session.status");
            assert_eq!(request["params"], json!({}));
            socket
                .send(Message::Text(
                    json!({
                        "type": "success",
                        "id": request["id"],
                        "result": {
                            "ready": false,
                            "message": "Browser is starting"
                        }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
        });

        let conn = Connection::connect(&format!("ws://{address}"))
            .await
            .unwrap();
        let status = get_status(&conn).await.unwrap();
        assert_eq!(
            status,
            BrowserStatus {
                ready: false,
                message: "Browser is starting".into(),
            }
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn network_subscription_receives_typed_before_request_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let request = socket.next().await.unwrap().unwrap();
            let Message::Text(request) = request else {
                panic!("expected BiDi command text");
            };
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "session.subscribe");
            assert_eq!(
                request["params"]["events"],
                json!(["network.beforeRequestSent"])
            );
            socket
                .send(Message::Text(
                    json!({
                        "type": "success",
                        "id": request["id"],
                        "result": {}
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({
                        "method": "network.beforeRequestSent",
                        "params": {
                            "request": {
                                "request": "request-1",
                                "url": "https://example.test/",
                                "method": "GET"
                            },
                            "redirectCount": 2
                        }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
        });

        let conn = Connection::connect(&format!("ws://{address}"))
            .await
            .unwrap();
        let subscribed = Mutex::new(false);
        let subscription = subscribe_network_before_request(&conn, &subscribed)
            .await
            .unwrap();
        let event = subscription.next(1_000).await.unwrap();

        assert_eq!(event.request_id(), "request-1");
        assert_eq!(event.url(), "https://example.test/");
        assert_eq!(event.method(), "GET");
        assert_eq!(event.redirect_count(), 2);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn network_subscription_reports_timeout_and_rejects_negative_timeout() {
        let (_sender, receiver) = broadcast::channel(1);
        let (closed_sender, closed) = tokio::sync::watch::channel(false);
        let subscription = NetworkEventSubscription {
            events: Mutex::new(receiver),
            closed,
        };

        assert_eq!(
            subscription.next(0).await.unwrap_err().kind,
            ErrorKind::Timeout
        );
        assert_eq!(
            subscription.next(-1).await.unwrap_err().kind,
            ErrorKind::InvalidArgument
        );
        closed_sender.send(true).unwrap();
        assert_eq!(
            subscription.next(10_000).await.unwrap_err().kind,
            ErrorKind::Protocol
        );
    }

    #[tokio::test]
    async fn launch_chrome_returns_error_for_missing_driver() {
        let options = ChromeOptions {
            driver_path: Some("/selenium-next-nonexistent-driver".into()),
            ..Default::default()
        };
        match Browser::launch_chrome(options).await {
            Err(error) => assert_eq!(error.kind, ErrorKind::LaunchFailed),
            Ok(_) => panic!("missing driver unexpectedly launched Chrome"),
        }
    }
}
