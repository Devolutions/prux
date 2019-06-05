use tokio::prelude::*;
use hyper::HeaderMap;
use hyper::header::AUTHORIZATION;
use serde_json::Value;
use reqwest::r#async::{Client, Response};
use base64::encode;
use std::net::Ipv4Addr;

pub struct HttpRequest {
    client: Client,
    headers: HeaderMap
}

impl HttpRequest {
    pub fn new(id: &str, password: &str) -> Self{
        let mut headers = HeaderMap::new();
        let encoded = encode(&format!("{}:{}", id, password));

        headers.append(AUTHORIZATION, format!("Basic {}", encoded).parse().unwrap());

        HttpRequest{
            client: Client::new(),
            headers
        }
    }

    pub fn lookup(&self, addr: &Ipv4Addr) -> impl Future<Item=Value, Error=()> {
        let json = |mut res : Response | {
            let result = res.json::<Value>().map_err(|_|());

            result
        };

        self.client
            .get(&format!("https://geoip.maxmind.com/geoip/v2.1/city/{}", addr))
            .headers(self.headers.clone())
            .send().map_err(|_| ())
            .and_then(json)
    }
}