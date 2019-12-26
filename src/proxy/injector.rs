use std::net::IpAddr;
use crate::IpResolver;
use std::collections::HashMap;

const PRUX_ADDR: &str = "Prux-Addr";
const PRUX_CITY: &str = "Prux-City";
const PRUX_COUNTRY: &str = "Prux-Country";
const PRUX_COORD: &str = "Prux-Coord"; // lat / long

pub async fn get_location_hdr(ip: IpAddr, resolver: IpResolver) -> Result<HashMap<String, String>, ()> {
    let json = resolver.lookup(&ip).await?;

    let mut hdr_map = HashMap::new();

    hdr_map.insert(PRUX_ADDR.to_string(), ip.to_string());

    if let Some(city_name_en) = json.get("city").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
        if let Some(name) = city_name_en {
            hdr_map.insert(PRUX_CITY.to_string(), name.to_string());
        }
    }

    if let Some(country_name_en) = json.get("country").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
        if let Some(name) = country_name_en {
            hdr_map.insert(PRUX_COUNTRY.to_string(), name.to_string());
        }
    }

    if let Some((Some(lat), Some(long))) = json.get("location").map(|val| {  (val.get("latitude").and_then(|l| l.as_f64().map(|n| n.to_string())), val.get("longitude").and_then(|l| l.as_f64().map(|n| n.to_string()))) }) {
        hdr_map.insert(PRUX_COORD.to_string(), format!("{},{}", lat, long));
    }

    Ok(hdr_map)
}
//
//pub async fn inject_basic_hdr(ip: IpAddr, resolver: IpResolver, mut request: Request<Body>) -> Result<Request<Body>, ()> {
//    let json = resolver.lookup(&ip).await?;
//
//    request.headers_mut().insert(HeaderName::from_bytes(PRUX_ADDR.as_bytes()).expect("should be ok"), HeaderValue::from_str(ip.to_string().as_str()).expect("should be ok"));
//
//    if let Some(city_name_en) = json.get("city").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
//        if let Some(name) = city_name_en {
//            request.headers_mut().insert(HeaderName::from_bytes(PRUX_CITY.as_bytes()).expect("should be ok"), HeaderValue::from_str(name).expect("should be ok"));
//        }
//    }
//
//    if let Some(country_name_en) = json.get("country").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
//        if let Some(name) = country_name_en {
//            request.headers_mut().insert(HeaderName::from_bytes(PRUX_COUNTRY.as_bytes()).expect("should be ok"), HeaderValue::from_str(name).expect("should be ok"));
//        }
//    }
//
//    if let Some((Some(lat), Some(long))) = json.get("location").map(|val| {  (val.get("latitude").and_then(|l| l.as_f64().map(|n| n.to_string())), val.get("longitude").and_then(|l| l.as_f64().map(|n| n.to_string()))) }) {
//        request.headers_mut().insert(HeaderName::from_bytes(PRUX_COORD.as_bytes()).expect("should be ok"), HeaderValue::from_str(&format!("{},{}", lat, long)).expect("should be ok"));
//    }
//
//    Ok(request)
//}
