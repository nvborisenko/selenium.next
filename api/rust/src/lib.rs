//! Synchronous Selenium.Next API.
//!
//! Browser operations block the calling thread. Dropping a browser or resource
//! handle attempts to close or remove the underlying resource. Explicit page
//! and context methods return cleanup errors.

mod events;
mod generated;

pub use events::NetworkRequestListener;
pub use generated::*;
pub use selenium_sync::{Error, ErrorKind, Result};
