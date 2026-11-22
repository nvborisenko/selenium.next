use crate::runtime;
use selenium_core as async_browser;
use selenium_core::Result;
use std::sync::Arc;

pub use selenium_core::ChromeOptions;

/// Launches Chrome synchronously. Browser operations block the calling thread.
pub struct Chrome;

impl Chrome {
    pub fn new(options: ChromeOptions) -> Result<Browser> {
        Browser::launch_chrome(options)
    }
}

#[derive(Clone)]
/// A browser whose operations block until they complete.
pub struct Browser(async_browser::Browser);

#[derive(Clone)]
pub struct UserContext {
    inner: async_browser::UserContext,
    pub id: String,
}

#[derive(Clone)]
pub struct Page {
    inner: async_browser::Page,
    pub id: String,
}

pub struct NetworkEventSubscription(Arc<async_browser::NetworkEventSubscription>);

pub struct NetworkBeforeRequest(async_browser::NetworkBeforeRequest);

impl From<async_browser::UserContext> for UserContext {
    fn from(inner: async_browser::UserContext) -> Self {
        Self {
            id: inner.id.clone(),
            inner,
        }
    }
}

impl From<async_browser::Page> for Page {
    fn from(inner: async_browser::Page) -> Self {
        Self {
            id: inner.id.clone(),
            inner,
        }
    }
}

impl Browser {
    pub fn launch_chrome(options: ChromeOptions) -> Result<Self> {
        runtime::run(async move {
            async_browser::Browser::launch_chrome(options)
                .await
                .map(Self)
        })
    }

    pub fn get_status(&self) -> Result<async_browser::BrowserStatus> {
        let browser = self.0.clone();
        runtime::run(async move { browser.get_status().await })
    }

    pub fn default_user_context(&self) -> Result<UserContext> {
        let browser = self.0.clone();
        runtime::run(async move { browser.default_user_context().await.map(UserContext::from) })
    }

    pub fn user_contexts(&self) -> Result<Vec<UserContext>> {
        let browser = self.0.clone();
        runtime::run(async move {
            browser
                .user_contexts()
                .await
                .map(|contexts| contexts.into_iter().map(UserContext::from).collect())
        })
    }

    pub fn create_user_context(&self) -> Result<UserContext> {
        let browser = self.0.clone();
        runtime::run(async move { browser.create_user_context().await.map(UserContext::from) })
    }

    pub fn default_page(&self) -> Result<Page> {
        let browser = self.0.clone();
        runtime::run(async move { browser.default_page().await.map(Page::from) })
    }

    pub fn navigate(&self, url: &str) -> Result<()> {
        let browser = self.0.clone();
        let url = url.to_owned();
        runtime::run(async move { browser.navigate(&url).await })
    }

    pub fn subscribe_network_before_request(&self) -> Result<NetworkEventSubscription> {
        let browser = self.0.clone();
        runtime::run(async move {
            browser
                .subscribe_network_before_request()
                .await
                .map(|subscription| NetworkEventSubscription(Arc::new(subscription)))
        })
    }

    pub fn close(&self) -> Result<()> {
        let browser = self.0.clone();
        runtime::run(async move { browser.close().await })
    }

    pub fn driver_port(&self) -> u16 {
        self.0.driver_port()
    }
}

impl UserContext {
    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut context = self.inner.clone();
        context.id.clone_from(&self.id);
        runtime::run(async move {
            context
                .pages()
                .await
                .map(|pages| pages.into_iter().map(Page::from).collect())
        })
    }

    pub fn new_page(&self) -> Result<Page> {
        let mut context = self.inner.clone();
        context.id.clone_from(&self.id);
        runtime::run(async move { context.new_page().await.map(Page::from) })
    }

    pub fn remove(&self) -> Result<()> {
        let mut context = self.inner.clone();
        context.id.clone_from(&self.id);
        runtime::run(async move { context.remove().await })
    }
}

impl Page {
    pub fn navigate(&self, url: &str) -> Result<()> {
        let mut page = self.inner.clone();
        page.id.clone_from(&self.id);
        let url = url.to_owned();
        runtime::run(async move { page.navigate(&url).await })
    }

    pub fn close(&self) -> Result<()> {
        let mut page = self.inner.clone();
        page.id.clone_from(&self.id);
        runtime::run(async move { page.close().await })
    }
}

impl NetworkEventSubscription {
    pub fn next(&self, timeout_ms: i32) -> Result<NetworkBeforeRequest> {
        let subscription = self.0.clone();
        runtime::run(async move {
            subscription
                .next(timeout_ms)
                .await
                .map(NetworkBeforeRequest)
        })
    }
}

impl NetworkBeforeRequest {
    pub fn request_id(&self) -> Result<String> {
        Ok(self.0.request_id().to_owned())
    }

    pub fn url(&self) -> Result<String> {
        Ok(self.0.url().to_owned())
    }

    pub fn method(&self) -> Result<String> {
        Ok(self.0.method().to_owned())
    }

    pub fn redirect_count(&self) -> Result<i32> {
        Ok(self.0.redirect_count())
    }
}
