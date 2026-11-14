use super::{
    adapter::adapt_page,
    models::{Board, Envelope, Page},
};
use crate::core::error::AppError;
use reqwest::{Client, Url};
use serde::de::DeserializeOwned;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub const API_BASE: &str = "https://skills.byted.org";
pub const SITE_BASE: &str = "https://skills.bytedance.net";
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;

static SHARED_CLIENT: Mutex<Option<CachedClient>> = Mutex::new(None);

struct CachedClient {
    proxy: Option<String>,
    client: Arc<MarketClient>,
}

pub struct MarketClient {
    pub http: Client,
    base: String,
}

impl MarketClient {
    pub fn shared(proxy: Option<&str>) -> Result<Arc<Self>, AppError> {
        let proxy = proxy.filter(|value| !value.trim().is_empty());
        let mut cached = SHARED_CLIENT.lock().map_err(AppError::internal)?;
        if let Some(existing) = cached.as_ref() {
            if existing.proxy.as_deref() == proxy {
                return Ok(Arc::clone(&existing.client));
            }
        }

        let client = Arc::new(Self::new(proxy)?);
        *cached = Some(CachedClient {
            proxy: proxy.map(str::to_owned),
            client: Arc::clone(&client),
        });
        Ok(client)
    }

    pub fn new(proxy: Option<&str>) -> Result<Self, AppError> {
        let mut builder = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }));
        if proxy.is_some() {
            let address = proxy.unwrap();
            if !address.trim().is_empty() {
                let proxy_config = reqwest::Proxy::all(address).map_err(AppError::network)?;
                builder = builder.proxy(proxy_config);
            }
        }
        Ok(Self {
            http: builder.build().map_err(AppError::network)?,
            base: API_BASE.into(),
        })
    }

    #[cfg(test)]
    pub fn for_test(base: String) -> Self {
        Self {
            http: Client::builder().no_proxy().build().unwrap(),
            base,
        }
    }

    pub async fn send(&self, url: Url, board: Board) -> Result<Page, AppError> {
        let response = self.json(url).await?;
        adapt_page(response, board)
    }

    pub fn detail_url(
        &self,
        identifier: &str,
        repository: &str,
        version: Option<&str>,
    ) -> Result<Url, AppError> {
        let path = match version {
            Some(version) => format!(
                "/api/v2/component/{}",
                urlencoding::encode(&format!("{identifier}:{version}"))
            ),
            None => format!("/api/v2/package/{}", urlencoding::encode(identifier)),
        };
        let mut url = self.url(&path)?;
        url.query_pairs_mut().append_pair("repository", repository);
        if version.is_some() {
            url.query_pairs_mut().append_pair("with_asset", "true");
        }
        Ok(url)
    }

    fn url(&self, path: &str) -> Result<Url, AppError> {
        Url::parse(&format!("{}{path}", self.base))
            .map_err(|error| AppError::invalid_input(error.to_string()))
    }

    pub async fn json<T: DeserializeOwned>(&self, url: Url) -> Result<T, AppError> {
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(AppError::network)?
            .error_for_status()
            .map_err(AppError::network)?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_JSON_BYTES as u64)
        {
            return Err(AppError::network("ByteDance response exceeds size limit"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(AppError::network)? {
            if bytes.len() + chunk.len() > MAX_JSON_BYTES {
                return Err(AppError::network("ByteDance response exceeds size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        // Read the business status first so permission failures remain readable.
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(AppError::network)?;
        ensure_success(value.get("message").and_then(|v| v.as_str()).unwrap_or(""))?;
        serde_json::from_value(value).map_err(AppError::network)
    }

    pub async fn detail<T: DeserializeOwned>(&self, url: Url) -> Result<T, AppError> {
        let response: Envelope<T> = self.json(url).await?;
        ensure_success(&response.message)?;
        Ok(response.data)
    }
}

pub fn ensure_success(message: &str) -> Result<(), AppError> {
    if message == "success" {
        Ok(())
    } else {
        Err(AppError::network(format!(
            "ByteDance marketplace: {message}"
        )))
    }
}

pub fn source_url(identifier: &str, repository: &str) -> String {
    let path = identifier
        .split('/')
        .map(|segment| urlencoding::encode(segment).replace("%3A", ":"))
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "{SITE_BASE}/skill/{path}?repository={}",
        urlencoding::encode(repository)
    )
}
