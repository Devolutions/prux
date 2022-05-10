use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use hyper::client::HttpConnector;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Body, Client, Request, Response, Uri};
use hyper_tls::HttpsConnector;
use log::error;

use crate::IpResolver;

const PRUX_ADDR: &str = "Prux-Addr";
const PRUX_CITY: &str = "Prux-City";
const PRUX_COUNTRY: &str = "Prux-Country";
const PRUX_COORD: &str = "Prux-Coord";
// lat / long
static IPV6_FORWARDED_TRIM_VALUE: &[char] = &['"', '[', ']'];

#[derive(Debug)]
pub struct StringError(pub String);

impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StringError {}

pub async fn get_location_hdr(
    ip: IpAddr,
    resolver: IpResolver,
) -> Result<HashMap<String, String>, ()> {
    let json = resolver.lookup(&ip).await?;

    let mut hdr_map = HashMap::new();

    hdr_map.insert(PRUX_ADDR.to_string(), ip.to_string());

    if let Some(Some(city_name_en)) = json
        .get("city")
        .and_then(|val| val.get("names"))
        .and_then(|names| names.get("en").map(|en_name| en_name.as_str()))
    {
        hdr_map.insert(PRUX_CITY.to_string(), city_name_en.to_string());
    }

    if let Some(Some(country_name_en)) = json
        .get("country")
        .and_then(|val| val.get("names"))
        .and_then(|names| names.get("en").map(|en_name| en_name.as_str()))
    {
        hdr_map.insert(PRUX_COUNTRY.to_string(), country_name_en.to_string());
    }

    if let Some((Some(lat), Some(long))) = json.get("location").map(|val| {
        (
            val.get("latitude")
                .and_then(|l| l.as_f64().map(|n| n.to_string())),
            val.get("longitude")
                .and_then(|l| l.as_f64().map(|n| n.to_string())),
        )
    }) {
        hdr_map.insert(PRUX_COORD.to_string(), format!("{},{}", lat, long));
    }

    Ok(hdr_map)
}

pub async fn gen_transmit_fut(
    client: &Client<HttpsConnector<HttpConnector>>,
    req: Request<Body>,
) -> Response<Body> {
    match client.request(req).await {
        Ok(response) => response,
        Err(e) => {
            error!("hyper error: {}", e);
            let mut response =
                Response::new(Body::from("Something went wrong, please try again later."));
            let (mut parts, body) = response.into_parts();
            parts.status = hyper::StatusCode::BAD_GATEWAY;
            response = Response::from_parts(parts, body);

            response
        }
    }
}

pub fn construct_request(
    request: Request<Body>,
    new_uri: Uri,
    headers: Option<HashMap<String, String>>,
) -> Request<Body> {
    let mut request = request;
    *request.uri_mut() = new_uri;

    if let Some(map) = headers {
        for (header, value) in map {
            request.headers_mut().insert(
                HeaderName::from_bytes(header.as_bytes()).expect("should be ok"),
                HeaderValue::from_str(value.as_str()).expect("should be ok"),
            );
        }
    }

    request
}

pub fn ip_is_global(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && !ip.is_unspecified()
        }
        IpAddr::V6(ip) => !ip.is_loopback() && !ip.is_unspecified(),
    }
}

pub fn get_forwarded_ip(req: &Request<Body>) -> Option<IpAddr> {
    let x_forwarded_ip: Option<String> = req
        .headers()
        .get("X-Forwarded-For")
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
        .map(|str_val| {
            str_val.split_once(',').map_or(&*str_val, |s| s.0).trim().to_lowercase()
        });

    let forwarded_ip = x_forwarded_ip.or_else(|| {
        req.headers()
            .get("Forwarded")
            .map(|value| String::from_utf8_lossy(value.as_bytes()))
            .and_then(|str_val| {
                str_val.to_lowercase().split(';').find_map(|s| {
                    s.split_once("for=")
                        .filter(|s| s.0.is_empty())
                        .map(|s| s.0)
                        .map(|s| s.split_once(',').map_or(s, |s| s.0).trim())
                        .map(|s| s.trim_matches(IPV6_FORWARDED_TRIM_VALUE).to_string())
                    // if s.starts_with("for=") {
                    //     s.splitn(2, '=').nth(1).and_then(|s| {
                    //         s.splitn(2, ", ")
                    //             .next()
                    //             .map(|s| s.trim_matches(IPV6_FORWARDED_TRIM_VALUE).to_string())
                    //     })
                    // } else {
                    //     None
                    // }
                })
            })
    });

    forwarded_ip.and_then(|ip_str| {
        Ipv4Addr::from_str(&ip_str)
            .map(IpAddr::V4)
            .ok()
            .or_else(|| Ipv6Addr::from_str(&ip_str).map(IpAddr::V6).ok())
    })
}
