pub mod injector;
use std::io;

use futures::{Future, Poll};
use std::net::{Ipv4Addr, SocketAddr};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::prelude::*;
use crate::IpResolver;
use crate::utils;
use bytes::{Buf, BufMut};
use ::{futures, httparse};
use std::io::Cursor;
use hyper::{Body, Request, Client, Response, Uri, StatusCode};
use hyper::http;
use hyper::client::{ResponseFuture, HttpConnector};
use hyper::header::{HeaderValue, HeaderName};
use hyper::service::Service;
use futures::future::{FutureResult, Either};
use hyper::http::Version;
use hashbrown::HashMap;
use httparse::Error;
use crate::utils::UriPathMatcher;

pub struct Proxy {
    pub upstream_addr: SocketAddr,
    pub source: Option<(Ipv4Addr, IpResolver)>,
    pub client: Client<HttpConnector>,
    pub path_inclusions: Vec<UriPathMatcher>,
    pub path_exclusions: Option<Vec<UriPathMatcher>>,
}

impl Proxy {
    pub fn new (upstream_addr: SocketAddr, source: Option<(Ipv4Addr, IpResolver)>, client: Client<HttpConnector>, inclusions: Vec<String>, exclusions: Option<Vec<String>>,) -> Self {
        Proxy {
            upstream_addr,
            source,
            client,
            path_inclusions: inclusions.iter().filter_map(|p| UriPathMatcher::new(p).map_err(|e| error!("Unable to construct included middleware route: {}", e)).ok()).collect(),
            path_exclusions: exclusions.map(|ex| ex.iter().filter_map(|p| UriPathMatcher::new(p).map_err(|e| error!("Unable to construct excluded middleware route: {}", e)).ok()).collect()),
        }
    }

    pub fn validate_path(&self, path: &str) -> bool {
        if self.path_inclusions.is_empty() {
            return true;
        }

        if self.path_inclusions.iter().any(|m_p| m_p.match_start(path)) {
            if let Some(ref path_exclusions) = self.path_exclusions {
                return !path_exclusions.iter().any(|m_e_p| m_e_p.match_start(path));
            } else {
                return true;
            }
        }

        false
    }

}

#[derive(Debug)]
pub struct StringError(String);

impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StringError {}

fn gen_transmit_fut(client: &Client<HttpConnector>, req: Request<Body>) -> impl Future<Item=Response<Body>, Error=StringError> + Send {
    client.request(req).map_err(|_| StringError("".to_string())).then(|result| {
        let our_response = match result.map_err(|_| StringError("".to_string())) {
            Ok(mut response) => {
                let version = match response.version() {
                    Version::HTTP_09 => "0.9",
                    Version::HTTP_10 => "1.0",
                    Version::HTTP_11 => "1.1",
                    Version::HTTP_2 => "2.0",
                    _ => "?",
                };
                {
                    let mut headers = response.headers_mut();

                    headers.append("proxy-info", HeaderValue::from_str(format!("{} prux-0.0.1", version).as_str()).expect("should be ok"));
                }

                response
            }
            Err(e) => {
                error!("hyper error: {}", e);
                let mut response = Response::new(Body::from("Something went wrong, please try again later."));
                let (mut parts, body) = response.into_parts();
                parts.status = StatusCode::BAD_GATEWAY;
                response = Response::from_parts(parts, body);

                response
            }
        };
        futures::future::ok(our_response)
    })
}

impl Service for Proxy {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = StringError;
    type Future = Box<Future<Item=Response<Body>, Error=StringError> + Send>;

    fn call(&mut self, req: Request<Self::ReqBody>) -> Self::Future {
        let last_part = if let Some(path) = req.uri().path_and_query() {
            path.as_str()
        } else {
            "/"
        };
        let upstream_uri = format!("http://{}{}", &self.upstream_addr.to_string(), last_part.to_string());
        let (mut parts, body) = req.into_parts();
        parts.uri = upstream_uri.parse().expect("Url must be valid");
        let mut outgoing_request = Request::from_parts(parts, body);
        if let Some(ipr) = self.source.clone() {
            if self.validate_path(outgoing_request.uri().path()) {
                let client = self.client.clone();
                Box::new(injector::inject_basic_hdr(ipr).map_err(|_| StringError("injection failed".to_string())).and_then(move |header_map| {
                    for (header, value) in header_map {
                        outgoing_request.headers_mut().insert(HeaderName::from_bytes(header.as_bytes()).expect("should be ok"), HeaderValue::from_str(value.as_str()).expect("should be ok"));
                    }
                    gen_transmit_fut(&client, outgoing_request)
                })) as Box<Future<Item=Response<Body>, Error=StringError> + Send>
            } else {
                Box::new(gen_transmit_fut(&self.client, outgoing_request)) as Box<Future<Item=Response<Body>, Error=StringError> + Send>
            }
        } else {
            Box::new(gen_transmit_fut(&self.client, outgoing_request)) as Box<Future<Item=Response<Body>, Error=StringError> + Send>
        }
    }
}