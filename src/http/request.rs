use crate::priority_map::PriorityMap;
use base64::encode;
use hyper::header::AUTHORIZATION;
use hyper::{Client, HeaderMap};
use serde_json::Value;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub struct Inner {
    pub client: Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>, hyper::Body>,
    pub headers: HeaderMap,
    pub cache: RwLock<PriorityMap<String, Arc<Value>>>,
}

#[derive(Clone)]
pub struct HttpRequest {
    inner: Arc<Inner>,
}

impl HttpRequest {
    pub fn new(id: &str, password: &str, cache_capacity: usize, cache_duration: Duration) -> Self {
        let mut headers = HeaderMap::new();
        let encoded = encode(format!("{}:{}", id, password));

        headers.append(
            AUTHORIZATION,
            format!("Basic {}", encoded).parse().expect("should be ok"),
        );

        let connector = hyper_tls::HttpsConnector::new();
        let client = Client::builder().build(connector);

        HttpRequest {
            inner: Arc::new(Inner {
                client,
                headers,
                cache: RwLock::new(PriorityMap::new(
                    cache_capacity,
                    cache_duration,
                    cache_duration,
                )),
            }),
        }
    }

    pub async fn lookup(&self, addr: &IpAddr) -> Result<Arc<Value>, ()> {
        let addr_str = format!("{}", addr);

        if !self.inner.cache.read().await.contains_key(&addr_str) {
            let mut req = hyper::Request::builder()
                .method(hyper::Method::GET)
                .uri(format!(
                    "https://geoip.maxmind.com/geoip/v2.1/city/{}",
                    addr_str
                ))
                .body(hyper::Body::empty())
                .map_err(|_| ())?;

            req.headers_mut()
                .extend(self.inner.headers.clone().into_iter());

            let res = self.inner.client.request(req).await.map_err(|_| ())?;

            let bytes = hyper::body::to_bytes(res.into_body())
                .await
                .map_err(|_| ())?;

            let json = serde_json::from_slice::<Value>(bytes.as_ref()).map_err(|_| ())?;

            self.inner
                .cache
                .write()
                .await
                .insert(addr_str.clone(), Arc::new(json));
        }

        self.inner
            .cache
            .read()
            .await
            .get(&addr_str)
            .cloned()
            .ok_or(())
    }
}
