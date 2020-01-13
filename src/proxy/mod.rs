use std::net::IpAddr;

use ::futures;
use futures::Future;
use futures::task::{Context, Poll};
use hyper::{Body, Client, Response, Uri};
use hyper::client::HttpConnector;
use hyper::service::Service;
use log::error;

use crate::IpResolver;
use crate::utils::UriPathMatcher;
use crate::proxy::utils::*;

pub mod utils;

pub struct Proxy {
    pub upstream_uri: Uri,
    pub source_ip: Option<IpAddr>,
    pub resolver: IpResolver,
    pub client: Client<HttpConnector>,
    pub path_inclusions: Vec<UriPathMatcher>,
    pub path_exclusions: Option<Vec<UriPathMatcher>>,
}

impl Proxy {
    pub fn new(upstream_uri: Uri, source_ip: Option<IpAddr>, resolver: IpResolver, client: Client<HttpConnector>, inclusions: Vec<String>, exclusions: Option<Vec<String>>) -> Self {
        Proxy {
            upstream_uri,
            source_ip,
            client,
            path_inclusions: inclusions.iter().filter_map(|p| UriPathMatcher::new(p).map_err(|e| error!("Unable to construct included middleware route: {}", e)).ok()).collect(),
            path_exclusions: exclusions.map(|ex| ex.iter().filter_map(|p| UriPathMatcher::new(p).map_err(|e| error!("Unable to construct excluded middleware route: {}", e)).ok()).collect()),
            resolver,
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

impl Service<hyper::Request<hyper::Body>> for Proxy {
    type Response = Response<Body>;
    type Error = StringError;
    type Future = Box<dyn Future<Output=Result<Response<Body>, Self::Error>> + Send + Unpin>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<hyper::Body>) -> Self::Future {
        let mut upstream_parts = self.upstream_uri.clone().into_parts();
        upstream_parts.path_and_query = req.uri().path_and_query().cloned();

        let upstream_uri = Uri::from_parts(upstream_parts).expect("Url must be valid");

        let forwarded_ip = get_forwarded_ip(&req).or_else(|| self.source_ip)
            .filter(|ip| ip_is_global(&ip) && self.validate_path(upstream_uri.path()));

        let client = self.client.clone();
        let resolver = self.resolver.clone();

        let fut = Box::pin(async move {
            let headers = if let Some(ip) = forwarded_ip {
                Some(utils::get_location_hdr(ip, resolver).await.map_err(|_| StringError("injection failed".to_string())))
            } else {
                None
            }.transpose();

            match headers {
                Ok(h) => {
                    let request = construct_request(req, upstream_uri, h);
                    Ok(gen_transmit_fut(&client, request).await)
                }
                Err(e) => {
                    Err(e)
                }
            }
        });

        Box::new(fut) as Box<dyn Future<Output=Result<Response<Body>, StringError>> + Send + Unpin>
    }
}