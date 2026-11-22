use selenium_next::{Chrome, ChromeOptions, ErrorKind};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn get_status_returns_readiness_and_message() -> selenium_next::Result<()> {
    let mut chrome = Chrome::with_options(ChromeOptions {
        headless: true,
        ..Default::default()
    })?;

    let status = chrome.get_status()?;

    assert!(!status.ready);
    assert!(!status.message.is_empty());
    chrome.close()?;
    Ok(())
}

#[test]
fn navigation_delivers_network_request_to_handler() -> selenium_next::Result<()> {
    let mut chrome = Chrome::with_options(ChromeOptions {
        headless: true,
        ..Default::default()
    })?;
    let (sender, receiver) = mpsc::channel();
    let caller_thread = std::thread::current().id();
    let listener = chrome.on_network_request(move |event| {
        sender
            .send((std::thread::current().id(), event))
            .expect("event receiver was dropped");
    })?;
    chrome.navigate("https://selenium.dev")?;

    let deadline = Instant::now() + Duration::from_secs(60);
    let callback_url = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (callback_thread, event) = receiver
            .recv_timeout(remaining)
            .expect("callback did not run");
        assert_ne!(callback_thread, caller_thread);
        let event = event?;
        if event.url.contains("selenium.dev") {
            break event.url;
        }
    };
    assert!(callback_url.contains("selenium.dev"));
    drop(listener);
    while receiver.try_recv().is_ok() {}
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    chrome.close()?;
    Ok(())
}

#[test]
fn dropping_page_and_context_closes_their_browser_resources() -> selenium_next::Result<()> {
    let mut chrome = Chrome::with_options(ChromeOptions {
        headless: true,
        ..Default::default()
    })?;
    let context = chrome.create_user_context()?;
    let page = context.new_page()?;
    drop(page);
    assert!(context.pages()?.is_empty());

    let page = context.new_page()?;
    drop(context);
    assert!(page.navigate("about:blank").is_err());
    assert!(page.close().is_err());
    chrome.close()?;
    Ok(())
}

#[test]
fn callback_reports_connection_closure() -> selenium_next::Result<()> {
    let mut chrome = Chrome::with_options(ChromeOptions {
        headless: true,
        ..Default::default()
    })?;
    let (sender, receiver) = mpsc::channel();
    let listener = chrome.on_network_request(move |event| {
        sender.send(event).expect("event receiver was dropped");
    })?;

    chrome.close()?;
    let error = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("callback did not report connection closure")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Protocol);
    drop(listener);
    Ok(())
}

#[test]
fn missing_driver_returns_launch_error() {
    let options = ChromeOptions {
        driver_path: Some("/selenium-next-nonexistent-driver".into()),
        ..Default::default()
    };
    match Chrome::with_options(options) {
        Err(error) => assert_eq!(error.kind, ErrorKind::LaunchFailed),
        Ok(_) => panic!("missing driver unexpectedly launched Chrome"),
    }
}
