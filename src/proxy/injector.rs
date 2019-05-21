use std::io;

use futures::{Future, Poll};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::prelude::*;
use crate::proxy::protocol::Protocol;
use std::net::Ipv4Addr;
use crate::IpResolver;

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

    let fut = resolver.lookup(&ip).and_then(move |json| {
        use std::io::Write;
        let mut hdr_vec = Vec::new();

        hdr_vec.as_mut_slice().write(PRUX_ADDR);
        hdr_vec.as_mut_slice().write(format!("{}", ip).as_bytes());
        hdr_vec.as_mut_slice().write(HDR_SEP);

        if let Some(city_name_en) = json.get("city").and_then(|val| val.get("name")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
            if let Some(name) = city_name_en {
                hdr_vec.as_mut_slice().write(PRUX_CITY);
                hdr_vec.as_mut_slice().write(name.as_bytes());
                hdr_vec.as_mut_slice().write(HDR_SEP);
            }
        }

        if let Some(country_name_en) = json.get("country").and_then(|val| val.get("name")).and_then(|names| names.get("en").map(|en_name| en_name.as_str())) {
            if let Some(name) = country_name_en {
                hdr_vec.as_mut_slice().write(PRUX_COUTRY);
                hdr_vec.as_mut_slice().write(name.as_bytes());
                hdr_vec.as_mut_slice().write(HDR_SEP);
            }
        }

        if let (Some(Some(lat)),Some(Some(long))) = (json.get("location").and_then(|val| val.get("latitude").map(|l| l.as_str())), json.get("location").and_then(|val| val.get("longitude").map(|l| l.as_str()))) {
            hdr_vec.as_mut_slice().write(PRUX_COORD);
            hdr_vec.as_mut_slice().write(lat.as_bytes());
            hdr_vec.as_mut_slice().write(b",");
            hdr_vec.as_mut_slice().write(long.as_bytes());
            hdr_vec.as_mut_slice().write(HDR_SEP);
        }

        future::finished(hdr_vec)
    });

    fut
}

