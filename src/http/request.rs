use tokio::prelude::*;
use hyper::HeaderMap;
use hyper::header::AUTHORIZATION;
use serde_json::Value;
use reqwest::r#async::{Client, Response};
use base64::encode;
use std::net::Ipv4Addr;
use parking_lot::RwLock;
use std::sync::Arc;
use crate::priority_map::PriorityMap;

pub struct Inner {
    pub client: Client,
    pub headers: HeaderMap,
    pub cache: RwLock<PriorityMap<String, Value>>,
}

#[derive(Clone)]
pub struct HttpRequest {
    inner: Arc<Inner>,
}

impl HttpRequest {
    pub fn new(id: &str, password: &str) -> Self {
        let mut headers = HeaderMap::new();
        let encoded = encode(&format!("{}:{}", id, password));

        headers.append(AUTHORIZATION, format!("Basic {}", encoded).parse().unwrap());

        HttpRequest {
            inner: Arc::new(Inner {
                client: Client::new(),
                headers,
                cache: RwLock::new(PriorityMap::new()),
            })
        }
    }

    pub fn lookup(&self, addr: &Ipv4Addr) -> impl Future<Item=Value, Error=()> {
        let self_lazy = self.clone();

        let addr_str = format!("{}", addr);

        let lazy = future::lazy(move || {
            self_lazy.inner.cache.read().get(&addr_str).map(|v| v.clone()).ok_or_else(move || addr_str).map_err(|addr| (self_lazy.clone(), addr))
        }).or_else(move |(self_req, addr)| {
            self_req.inner.client
                .get(&format!("https://geoip.maxmind.com/geoip/v2.1/city/{}", addr))
                .headers(self_req.inner.headers.clone())
                .send().map_err(|_| ())
                .and_then(|mut res: Response| {
                    let result = res.json::<Value>().map_err(|_| ());
                    result
                }).inspect(move |value| {
                self_req.inner.cache.write().insert(addr, value.clone());
            })
        });

        lazy
    }
}