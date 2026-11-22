//! Blocking adapter for the asynchronous Selenium core.
//!
//! Operations block the calling thread, including when called from an async runtime.

mod browser;
mod runtime;

pub use browser::{
    Browser, Chrome, ChromeOptions, NetworkBeforeRequest, NetworkEventSubscription, Page,
    UserContext,
};
pub use selenium_core::{BrowserStatus, Error, ErrorKind, Result};
