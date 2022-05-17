use std::collections::HashMap;
use std::net::IpAddr;

use ::futures;
use futures::task::{Context, Poll};
use futures::Future;
use hyper::client::HttpConnector;
use hyper::service::Service;
use hyper::{Body, Client, Response, Uri};
use hyper_tls::HttpsConnector;
use log::error;

use crate::proxy::utils::*;
use crate::utils::UriPathMatcher;
use crate::IpResolver;

pub mod utils;

pub struct Proxy {
    pub upstream_uri: Uri,
    pub source_ip: Option<IpAddr>,
    pub resolver: IpResolver,
    pub client: Client<HttpsConnector<HttpConnector>>,
    pub ip_path_inclusions: Vec<UriPathMatcher>,
    pub maxmind_path_inclusions: Vec<UriPathMatcher>,
    pub path_exclusions: Option<Vec<UriPathMatcher>>,
}

impl Proxy {
    pub fn new(
        upstream_uri: Uri,
        source_ip: Option<IpAddr>,
        resolver: IpResolver,
        client: Client<HttpsConnector<HttpConnector>>,
        ip_inclusions: Vec<String>,
        maxmind_inclusions: Vec<String>,
        exclusions: Option<Vec<String>>,
    ) -> Self {
        Proxy {
            upstream_uri,
            source_ip,
            client,
            ip_path_inclusions: ip_inclusions
                .iter()
                .filter_map(|p| {
                    UriPathMatcher::new(p)
                        .map_err(|e| error!("Unable to construct included middleware route: {}", e))
                        .ok()
                })
                .collect(),
            maxmind_path_inclusions: maxmind_inclusions
                .iter()
                .filter_map(|p| {
                    UriPathMatcher::new(p)
                        .map_err(|e| error!("Unable to construct included middleware route: {}", e))
                        .ok()
                })
                .collect(),
            path_exclusions: exclusions.map(|ex| {
                ex.iter()
                    .filter_map(|p| {
                        UriPathMatcher::new(p)
                            .map_err(|e| {
                                error!("Unable to construct excluded middleware route: {}", e)
                            })
                            .ok()
                    })
                    .collect()
            }),
            resolver,
        }
    }

    pub fn validate_ip_path(&self, path: &str) -> bool {
        if self.ip_path_inclusions.is_empty() {
            return true;
        }

        if self
            .ip_path_inclusions
            .iter()
            .any(|m_p| m_p.match_start(path))
        {
            if let Some(ref path_exclusions) = self.path_exclusions {
                return !path_exclusions.iter().any(|m_e_p| m_e_p.match_start(path));
            } else {
                return true;
            }
        }

        false
    }

    pub fn validate_maxmind_path(&self, path: &str) -> bool {
        if self.maxmind_path_inclusions.is_empty() {
            return true;
        }

        if self
            .maxmind_path_inclusions
            .iter()
            .any(|m_p| m_p.match_start(path))
        {
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
    type Future = Box<dyn Future<Output = Result<Response<Body>, Self::Error>> + Send + Unpin>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<hyper::Body>) -> Self::Future {
        let mut upstream_parts = self.upstream_uri.clone().into_parts();
        upstream_parts.path_and_query = req.uri().path_and_query().cloned();

        let upstream_uri = Uri::from_parts(upstream_parts).expect("Url must be valid");

        let valid_maxmind = self.validate_maxmind_path(upstream_uri.path());
        let valid_ip = self.validate_ip_path(upstream_uri.path());

        let forwarded_ip = get_forwarded_ip(&req)
            .or(self.source_ip)
            .filter(ip_is_global);

        let client = self.client.clone();
        let resolver = self.resolver.clone();

        let fut = Box::pin(async move {
            let headers = if let Some(ip) = forwarded_ip {
                let mut hdr_map = HashMap::new();
                if valid_ip || valid_maxmind {
                    utils::add_ip_hdr(&ip, &mut hdr_map).await;
                }

                if valid_maxmind {
                    utils::get_location_hdr(ip, resolver, &mut hdr_map)
                        .await
                        .map_err(|_| StringError("injection failed".to_string()))?;
                }

                Some(hdr_map)
            } else {
                None
            };

            let request = construct_request(req, upstream_uri, headers);
            Ok(gen_transmit_fut(&client, request).await)
        });

        Box::new(fut)
            as Box<dyn Future<Output = Result<Response<Body>, StringError>> + Send + Unpin>
    }
}
