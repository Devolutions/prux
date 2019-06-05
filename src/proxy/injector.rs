use futures::Future;
use tokio::prelude::*;
use std::net::Ipv4Addr;
use crate::IpResolver;
use serde_json::Value;
use bytes::BufMut;

const PRUX_ADDR: &[u8] = b"Prux-Addr: ";
const PRUX_CITY: &[u8] = b"Prux-City: ";
const PRUX_COUTRY: &[u8] = b"Prux-Country: ";
const PRUX_CONTINENT: &[u8] = b"Prux-Continent: ";
const PRUX_TIMEZONE: &[u8] = b"Prux-Timezone: ";
const PRUX_COORD: &[u8] = b"Prux-Coord: "; // lat / long
const PRUX_RADIUS: &[u8] = b"Prux-Radius: ";
const PRUX_SUB: &[u8] = b"Prux-Sub: "; // Province
const PRUX_ISP: &[u8] = b"Prux-Isp: ";
pub const HDR_SEP: &[u8] = b"\r\n";


pub fn inject_basic_hdr(ipr: (Ipv4Addr, IpResolver)) -> impl Future<Item=Vec<u8>, Error=()> {
    let (ip, resolver) = ipr;

    let fut = resolver.lookup(&ip).and_then(move |json: Value| {
        use std::io::Write;
        let mut hdr_vec = Vec::new().writer();

        hdr_vec.write(PRUX_ADDR);
        hdr_vec.write(ip.to_string().as_bytes());
        hdr_vec.write(HDR_SEP);

        if let Some(city_name_en) = json.get("city").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
            if let Some(name) = city_name_en {
                hdr_vec.write(PRUX_CITY);
                hdr_vec.write(name.as_bytes());
                hdr_vec.write(HDR_SEP);
            }
        }

        if let Some(country_name_en) = json.get("country").and_then(|val| val.get("names")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
            if let Some(name) = country_name_en {
                hdr_vec.write(PRUX_COUTRY);
                hdr_vec.write(name.as_bytes());
                hdr_vec.write(HDR_SEP);
            }
        }

        if let Some((Some(lat), Some(long))) = json.get("location").map(|val| {  (val.get("latitude").and_then(|l| l.as_f64().map(|n| n.to_string())), val.get("longitude").and_then(|l| l.as_f64().map(|n| n.to_string()))) }) {
            hdr_vec.write(PRUX_COORD);
            hdr_vec.write(lat.as_bytes());
            hdr_vec.write(b",");
            hdr_vec.write(long.as_bytes());
            hdr_vec.write(HDR_SEP);
        }

        future::finished(hdr_vec.into_inner())
    });

    fut
}

