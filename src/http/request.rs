use std::net::IpAddr;
use std::sync::Arc;

use base64::encode;
use hyper::{Client, HeaderMap};
use hyper::header::AUTHORIZATION;
use serde_json::Value;
use futures::lock::Mutex;

use crate::priority_map::PriorityMap;

pub struct Inner {
    pub client: Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>, hyper::Body>,
    pub headers: HeaderMap,
    pub cache: Mutex<PriorityMap<String, Value>>,
}

#[derive(Clone)]
pub struct HttpRequest {
    inner: Arc<Inner>,
}

impl HttpRequest {
    pub fn new(id: &str, password: &str, cache_capacity: usize) -> Self {
        let mut headers = HeaderMap::new();
        let encoded = encode(&format!("{}:{}", id, password));

        headers.append(AUTHORIZATION, format!("Basic {}", encoded).parse().expect("should be ok"));

        let connector = hyper_tls::HttpsConnector::new();
        let client = Client::builder().build(connector);

        HttpRequest {
            inner: Arc::new(Inner {
                client,
                headers,
                cache: Mutex::new(PriorityMap::new(cache_capacity)),
            })
        }
    }

    pub async fn lookup(&self, addr: &IpAddr) -> Result<Value, ()> {
        let addr_str = format!("{}", addr);

        let mut cache = self.inner.cache.lock().await;

        if let Some(value) = cache.get(&addr_str) {
            Ok(value.clone())
        } else {
            let mut req = hyper::Request::builder()
                .method(hyper::Method::GET)
                .uri(format!("https://geoip.maxmind.com/geoip/v2.1/city/{}", addr_str))
                .body(hyper::Body::empty()).map_err(|_| ())?;

            for (h_name, h_val) in &self.inner.headers {
                req.headers_mut().append(h_name.clone(), h_val.clone());
            }

            let res = self.inner.client.request(req).await.map_err(|_| ())?;

            let bytes = hyper::body::to_bytes(res.into_body()).await.map_err(|_| ())?;

            let json = serde_json::from_slice::<Value>(bytes.as_ref()).map_err(|_| ())?;

            cache.insert(addr_str, json.clone());

            Ok(json)
        }
    }
}