use std::net::{Ipv4Addr, IpAddr, Ipv6Addr};

use ::futures;
use futures::Future;
use hyper::{Body, Client, Request, Response, StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::header::{HeaderName, HeaderValue};
use hyper::http::Version;
use hyper::service::Service;
use log::error;

use crate::IpResolver;
use crate::utils::UriPathMatcher;

pub mod injector;

static IPV6_FORWARDED_TRIM_VALUE: &'static [char] = &['"', '[', ']'];

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
                };
                {
                    let headers = response.headers_mut();

                    headers.append("proxy-info", HeaderValue::from_str(format!("{} prux-1.1.0", version).as_str()).expect("should be ok"));
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
    type Future = Box<dyn Future<Item=Response<Body>, Error=StringError> + Send>;

    fn call(&mut self, req: Request<Self::ReqBody>) -> Self::Future {
        use std::str::FromStr;

        let mut upstream_parts = self.upstream_uri.clone().into_parts();
        upstream_parts.path_and_query = req.uri().path_and_query().cloned();

        let upstream_uri = Uri::from_parts(upstream_parts).expect("Url must be valid");

        let x_forwarded_ip: Option<String> = req.headers().get("X-Forwarded-For").map(|value| String::from_utf8_lossy(value.as_bytes())).and_then(|str_val| str_val.to_lowercase().splitn(2, ", ").next().map(|s| s.to_string()));

        let forwarded_ip = x_forwarded_ip.or_else(|| req.headers().get("Forwarded").map(|value| String::from_utf8_lossy(value.as_bytes())).and_then(|str_val| {
            str_val.to_lowercase().split(';').find_map(|s| {
                if s.starts_with("for=") {
                    s.splitn(2, "=").skip(1).next().and_then(|s| s.splitn(2, ", ").next().map(|s| s.trim_matches(IPV6_FORWARDED_TRIM_VALUE).to_string()))
                } else {
                    None
                }
            })
        }));

        let forwarded_ip = forwarded_ip.and_then(|ip_str| Ipv4Addr::from_str(&ip_str).map(|ip| IpAddr::V4(ip)).ok().or_else(|| Ipv6Addr::from_str(&ip_str).map(|ip| IpAddr::V6(ip)).ok())).or(self.source_ip.clone());

        let mut outgoing_request = req;
        *outgoing_request.uri_mut() = upstream_uri;

        if let Some(ip) = forwarded_ip {
            if ip_is_global(&ip) && self.validate_path(outgoing_request.uri().path()) {
                let client = self.client.clone();
                Box::new(injector::inject_basic_hdr(ip, self.resolver.clone()).map_err(|_| StringError("injection failed".to_string())).and_then(move |header_map| {
                    for (header, value) in header_map {
                        outgoing_request.headers_mut().insert(HeaderName::from_bytes(header.as_bytes()).expect("should be ok"), HeaderValue::from_str(value.as_str()).expect("should be ok"));
                    }
                    gen_transmit_fut(&client, outgoing_request)
                })) as Box<dyn Future<Item=Response<Body>, Error=StringError> + Send>
            } else {
                Box::new(gen_transmit_fut(&self.client, outgoing_request)) as Box<dyn Future<Item=Response<Body>, Error=StringError> + Send>
            }
        } else {
            Box::new(gen_transmit_fut(&self.client, outgoing_request)) as Box<dyn Future<Item=Response<Body>, Error=StringError> + Send>
        }
    }
}

pub fn ip_is_global(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !ip.is_private() && !ip.is_loopback() && !ip.is_link_local() &&
            !ip.is_broadcast() && !ip.is_documentation() && !ip.is_unspecified(),
        IpAddr::V6(ip) => !ip.is_loopback() && !ip.is_unspecified(),
    }
}