#[macro_use]
extern crate serde_derive;

use std::env;
use std::io;
use std::net;
use std::net::SocketAddr;
use std::time::Duration;

use dns_lookup::lookup_host;
use env_logger::Builder;
use hyper::server::conn::Http;
use hyper::{Client, Uri};
use hyper_tls::HttpsConnector;
use log::LevelFilter;
use tokio::net::TcpListener;

use crate::http::request::HttpRequest;
use crate::proxy::Proxy;

pub type IpResolver = HttpRequest;

mod http;
mod priority_map;
mod proxy;
mod settings;
mod utils;

#[tokio::main]
async fn main() -> io::Result<()> {
    let config = settings::Settings::load().expect("Configuration errors are fatal");

    let mut builder = Builder::new();
    builder.filter(None, config.level_filter());
    builder.filter(Some("tokio_io"), LevelFilter::Off);
    builder.filter(Some("tokio_core"), LevelFilter::Off);
    builder.filter(Some("tokio_reactor"), LevelFilter::Off);
    builder.filter(Some("tokio_threadpool"), LevelFilter::Off);
    builder.filter(Some("mio"), LevelFilter::Off);
    builder.filter(Some("hyper"), LevelFilter::Off);

    if let Ok(rust_log) = env::var("RUST_LOG") {
        builder.parse_filters(&rust_log);
    }

    builder.init();

    let ip_resolver = HttpRequest::new(
        &config.server.maxmind_id,
        &config.server.maxmind_password,
        config.server.cache_capacity,
        Duration::from_secs(config.server.cache_duration_secs),
    );

    let server_uri = config
        .server
        .uri
        .as_str()
        .parse::<Uri>()
        .expect("Invalid upstream uri");

    let listener =
        TcpListener::bind((net::Ipv4Addr::new(0, 0, 0, 0), config.listener.port)).await?;

    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let http = Http::new();

    while let Ok((stream, addr)) = listener.accept().await {
        let client_hpr = client.clone();
        let server_uri = server_uri.clone();
        let resolver = ip_resolver.clone();
        let source = addr.ip();

        let inclusions = config
            .server
            .path_inclusions
            .split(',')
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        let exclusions = if let Some(exclusion_string) = config.server.path_exclusions.clone() {
            exclusion_string
                .split(',')
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        } else {
            Vec::new()
        };

        let http_proxy = http.serve_connection(
            stream,
            Proxy::new(
                server_uri,
                Some(source),
                resolver,
                client_hpr,
                inclusions,
                Some(exclusions),
                config.server.forwarded_ip_header.clone(),
                config.server.use_forwarded_ip_header_only,
            ),
        );

        tokio::spawn(http_proxy);
    }

    Ok(())
}

pub fn sockaddr_from_uri(uri: &str) -> Result<SocketAddr, String> {
    let uri: Uri = uri
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;

    let ip = {
        if let Ok(addrs) = get_addr_from_uri(&uri) {
            if let Some(ipv4) = addrs.iter().find(|ip| ip.is_ipv4()) {
                *ipv4
            } else {
                return Err(String::from("No local ipV4Addr specified"));
            }
        } else {
            return Err(String::from("No local ipAddr specified"));
        }
    };

    let port = match uri.port_u16() {
        Some(p) => p,
        None => return Err("No port specified".to_string()),
    };

    Ok(SocketAddr::new(ip, port))
}

fn get_addr_from_uri(uri: &Uri) -> Result<Vec<net::IpAddr>, String> {
    if let Some(host) = uri.host() {
        let ips: Vec<net::IpAddr> = match lookup_host(host) {
            Ok(hosts) => hosts,
            Err(_) => return Err("Unable to lookup host".to_string()),
        };

        return Ok(ips);
    }

    Err("No host specified".to_string())
}
