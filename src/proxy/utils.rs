use hyper::client::HttpConnector;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Body, Client, HeaderMap, Request, Response, Uri};
use hyper_tls::HttpsConnector;
use log::error;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use crate::IpResolver;

const PRUX_ADDR: &str = "Prux-Addr";
const PRUX_CITY: &str = "Prux-City";
const PRUX_COUNTRY: &str = "Prux-Country";
const PRUX_PROVINCE: &str = "Prux-Province";
const PRUX_COORD: &str = "Prux-Coord";
const PRUX_COORD_ACCURACY: &str = "Prux-Coord-Accuracy";
const PRUX_TIMEZONE: &str = "Prux-Timezone";
const PRUX_ISP: &str = "Prux-ISP";
const PRUX_NETWORK: &str = "Prux-Network";
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

pub async fn add_ip_hdr(ip: &IpAddr, hdr_map: &mut HashMap<String, String>) {
    hdr_map.insert(PRUX_ADDR.to_string(), ip.to_string());
}

pub async fn get_location_hdr(
    ip: IpAddr,
    resolver: IpResolver,
    hdr_map: &mut HashMap<String, String>,
) -> Result<(), ()> {
    let json = resolver.lookup(&ip).await?;
    dbg!(&json);

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

    if let Some(loc) = json.get("location") {
        if let (Some(lat), Some(long)) = (
            loc.get("latitude").and_then(|l| l.as_f64()),
            loc.get("longitude").and_then(|l| l.as_f64()),
        ) {
            hdr_map.insert(PRUX_COORD.to_string(), format!("{},{}", lat, long));
        }

        if let Some(acc) = loc.get("accuracy_radius").and_then(|acc| acc.as_f64()) {
            hdr_map.insert(PRUX_COORD_ACCURACY.to_string(), format!("{acc:.0}"));
        }

        if let Some(loc) = loc.get("time_zone").and_then(|tz| tz.as_str()) {
            hdr_map.insert(PRUX_TIMEZONE.to_string(), loc.to_string());
        }
    }

    if let Some(iso) = json
        .get("subdivisions")
        .and_then(|sub| sub.get("iso_code"))
        .and_then(|iso| iso.as_str())
    {
        hdr_map.insert(PRUX_PROVINCE.to_string(), iso.to_string());
    }

    if let Some(traits) = json.get("traits") {
        if let Some(isp) = traits.get("isp").and_then(|isp| isp.as_str()) {
            hdr_map.insert(PRUX_ISP.to_string(), isp.to_string());
        }

        if let Some(network) = traits.get("network").and_then(|network| network.as_str()) {
            hdr_map.insert(PRUX_NETWORK.to_string(), network.to_string());
        }
    }

    Ok(())
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

pub fn get_forwarded_ip(
    req: &Request<Body>,
    forwarded_ip_header: Option<&str>,
    use_forwarded_ip_header_only: bool,
) -> Option<IpAddr> {
    get_forwarded_ip_from_headers(
        req.headers(),
        forwarded_ip_header,
        use_forwarded_ip_header_only,
    )
}

fn get_forwarded_ip_from_headers(
    headers: &HeaderMap,
    forwarded_ip_header: Option<&str>,
    use_forwarded_ip_header_only: bool,
) -> Option<IpAddr> {
    let mut ip_str = forwarded_ip_header.and_then(|header| {
        headers
            .get(header)
            .map(|value| String::from_utf8_lossy(value.as_bytes()))
            .map(|str_val| str_val.trim().to_lowercase())
    });

    if !use_forwarded_ip_header_only {
        ip_str = ip_str
            .or_else(|| get_ip_str_from_x_forwarded_header(headers))
            .or_else(|| get_ip_str_from_forwarded_header(headers));
    }

    ip_str.and_then(|ip_str| {
        Ipv4Addr::from_str(&ip_str)
            .map(IpAddr::V4)
            .ok()
            .or_else(|| Ipv6Addr::from_str(&ip_str).map(IpAddr::V6).ok())
    })
}

fn get_ip_str_from_x_forwarded_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Forwarded-For")
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
        .map(|str_val| {
            str_val
                .split_once(',')
                .map_or(&*str_val, |s| s.0)
                .trim()
                .trim_matches(IPV6_FORWARDED_TRIM_VALUE)
                .to_lowercase()
        })
}

fn get_ip_str_from_forwarded_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Forwarded")
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
        .and_then(|str_val| {
            str_val.to_lowercase().split(';').find_map(|s| {
                s.split_once("for=")
                    .map(|s| s.1)
                    .map(|s| s.split_once(',').map_or(s, |s| s.0).trim())
                    .map(|s| s.trim_matches(IPV6_FORWARDED_TRIM_VALUE).to_string())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::get_forwarded_ip_from_headers;
    use hyper::header::{HeaderName, HeaderValue};
    use hyper::{header, HeaderMap};
    use std::net::IpAddr;
    use std::str::FromStr;

    fn build_test_header(forwarded: Option<&str>, x_forwarded: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::with_capacity(2);

        if let Some(f) = forwarded {
            headers.insert(header::FORWARDED, HeaderValue::from_str(f).unwrap());
        }

        if let Some(f) = x_forwarded {
            headers.insert(
                HeaderName::from_str("X-Forwarded-For").unwrap(),
                HeaderValue::from_str(f).unwrap(),
            );
        }

        headers
    }

    #[test]
    fn test_forwarded() {
        let forwarded = "for=192.0.2.43";
        let headers = build_test_header(Some(forwarded), None);
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.43").ok(),
            r#"testing simple ipv4 Forwarded header : "Fowrarded: {}""#,
            forwarded
        );

        let forwarded = r#"for="[2001:db8:cafe::17]""#;
        let headers = build_test_header(Some(forwarded), None);
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("2001:db8:cafe::17").ok(),
            r#"testing simple ipv6 Forwarded header : "Fowrarded: {}""#,
            forwarded
        );

        let forwarded = r#"for=192.0.2.44, for="[2001:db8:cafe::17]""#;
        let headers = build_test_header(Some(forwarded), None);
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.44").ok(),
            r#"testing Forwarded header with multiple for : "Fowrarded: {}""#,
            forwarded
        );

        let forwarded = r#"for=192.0.2.45  ,  for="[2001:db8:cafe::17]""#;
        let headers = build_test_header(Some(forwarded), None);
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.45").ok(),
            r#"testing Forwarded header with multiple for and whitespaces : "Fowrarded: {}""#,
            forwarded
        );

        let forwarded = r#"by=203.0.113.42;for=192.0.2.46, for="[2001:db8:cafe::17]""#;
        let headers = build_test_header(Some(forwarded), None);
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.46").ok(),
            r#"testing Forwarded header "by" clause : "Fowrarded: {}""#,
            forwarded
        );
    }

    #[test]
    fn x_forwarded_for() {
        let x_forwarded_for = "192.0.2.43";
        let headers = build_test_header(None, Some(x_forwarded_for));
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.43").ok(),
            r#"testing simple ipv4 X-Forwarded-For header : "X-Fowrarded-For: {}""#,
            x_forwarded_for
        );

        let x_forwarded_for = r#"192.0.2.44, "[2001:db8:cafe::17]""#;
        let headers = build_test_header(None, Some(x_forwarded_for));
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.44").ok(),
            r#"testing simple ipv4 X-Forwarded-For header with proxies : "X-Fowrarded-For: {}""#,
            x_forwarded_for
        );

        let x_forwarded_for = r#"2001:db8:cafe::17"#;
        let headers = build_test_header(None, Some(x_forwarded_for));
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("2001:db8:cafe::17").ok(),
            r#"testing simple ipv6 X-Forwarded-For header : "X-Fowrarded-For: {}""#,
            x_forwarded_for
        );

        let x_forwarded_for = r#""[2001:db8:cafe::17]""#;
        let headers = build_test_header(None, Some(x_forwarded_for));
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("2001:db8:cafe::17").ok(),
            r#"testing simple ipv6 X-Forwarded-For header with "Forwarded"-style delimiters : "X-Fowrarded-For: {}""#,
            x_forwarded_for
        );
    }

    #[test]
    fn x_forwarded_for_priority() {
        let forwarded = r#"by=203.0.113.42;for=192.0.2.46, for="[2001:db8:cafe::18]""#;
        let x_forwarded_for = r#"192.0.2.44, "[2001:db8:cafe::17]""#;
        let headers = build_test_header(Some(forwarded), Some(x_forwarded_for));
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, None, false),
            IpAddr::from_str("192.0.2.44").ok(),
            "Testing \"X-Fowrarded-For\" priority over \"Forwarded\"; Headers: \n\"X-Forwarded-For: {}\"\n\"Forwarded: {}\"",
            x_forwarded_for,
            forwarded
        );
    }

    #[test]
    fn custom_ip_forwarding_header() {
        let ip = r#"203.0.113.42"#;
        let header_name = Some("CF-Connecting-IP".to_string());
        let mut headers = HeaderMap::with_capacity(1);
        headers.insert(
            HeaderName::from_str(header_name.as_deref().unwrap()).unwrap(),
            HeaderValue::from_str(ip).unwrap(),
        );
        assert_eq!(
            get_forwarded_ip_from_headers(&headers, header_name.as_deref(), false),
            IpAddr::from_str(ip).ok(),
            r#"Testing custom forwarded ip header with header name "CF-Connecting-IP""#,
        );
    }
}
