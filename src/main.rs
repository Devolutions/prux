extern crate tokio;
extern crate tokio_io;
extern crate tokio_tcp;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;
extern crate serde;
extern crate toml;
extern crate config;
#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate clap;
extern crate dns_lookup;
extern crate hyper;
extern crate parking_lot;
extern crate reqwest;
extern crate base64;

use hyper::{Uri, Server, Client};
use env_logger::Builder;
use log::LevelFilter;
use std::env;
use std::net::{SocketAddr, Ipv4Addr};
use std::net;
use dns_lookup::lookup_host;
use tokio_tcp::TcpListener;
use tokio::prelude::*;
use tokio::io::shutdown;
use tokio_tcp::TcpStream;

use parking_lot::Mutex;
use std::net::Shutdown;
use crate::http::request::HttpRequest;
use crossbeam_channel::bounded;
use crate::proxy::{Proxy};
use futures::sync::oneshot;
use futures::task::Task;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use hyper::server::conn::Http;

static counter: AtomicUsize = AtomicUsize::new(0);

pub type IpResolver = HttpRequest;

mod settings;
mod proxy;
mod http;
mod utils;
mod priority_map;

pub fn ipv4addr_is_global(ip: &std::net::Ipv4Addr) -> bool {
    !ip.is_private() && !ip.is_loopback() && !ip.is_link_local() &&
        !ip.is_broadcast() && !ip.is_documentation() && !ip.is_unspecified()
}

fn main() {
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
        builder.parse(&rust_log);
    }

    builder.init();

    let ip_resolver = HttpRequest::new(&config.server.maxmind_id, &config.server.maxmind_password);

    let server_addr = sockaddr_from_uri(config.server.uri.as_str()).unwrap();

    let addr = (net::Ipv4Addr::new(0, 0, 0, 0), config.listener.port).into();
    let listener = TcpListener::bind(&addr).unwrap();

    let client = Client::new();

    let http = Http::new();

    let done = listener.incoming()
            .map_err(|e| error!("error accepting socket; error = {:?}", e))
            .for_each(move |client_socket| {
                let ipr = ip_resolver.clone();
                let client_hpr = client.clone();
                let client_addr = client_socket.peer_addr();
                let closure = |_| ();
                let http_proxy = match client_addr {
                    Ok(std::net::SocketAddr::V4(ip)) if ipv4addr_is_global(ip.ip()) => {
                        let inclusions = config.server.path_inclusions.split(",").map(|s| s.to_string()).collect::<Vec<String>>();
                        let exclusions = if let Some(exclusion_string) = config.server.path_exclusions.clone() {
                            exclusion_string.split(",").map(|s| s.to_string()).collect::<Vec<String>>()
                        } else {
                            Vec::new()
                        };

                        http.serve_connection(client_socket, Proxy::new(
                            server_addr,
                            Some((ip.ip().clone(), ipr)),
                            client_hpr,
                            inclusions,
                            Some(exclusions),

                        )).map_err(closure)
                    }
                    _ => {
                        http.serve_connection(client_socket, Proxy {
                            upstream_addr: server_addr,
                            source: None,
                            client: client_hpr,
                            path_inclusions: Vec::new(),
                            path_exclusions: None,
                        }).map_err(closure)
                    }
                };

                tokio::spawn(http_proxy)
            });

    tokio::run(done);
}

pub fn sockaddr_from_uri(uri: &str) -> Result<SocketAddr, String> {
    let uri: Uri = uri.parse().map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;
    let port;

    let ip = {
        if let Ok(addrs) = get_addr_from_uri(&uri) {
            if let Some(ipv4) = addrs.iter().find(|ip| ip.is_ipv4()) {
                ipv4.clone()
            } else {
                return Err(String::from(
                    "No local ipV4Addr specified"));
            }
        } else {
            return Err(String::from(
                "No local ipAddr specified"));
        }
    };

    if let Some(p) = uri.port_part() {
        port = p.as_u16();
    } else {
        return Err("colisse".to_string());
    }

    Ok(SocketAddr::new(ip, port))
}

fn get_addr_from_uri(uri: &Uri) -> Result<Vec<net::IpAddr>, String> {
    if let Some(host) = uri.host() {
        let ips: Vec<net::IpAddr> = match lookup_host(host) {
            Ok(hosts) => hosts,
            Err(_) => return Err("colisse".to_string()),
        };

        return Ok(ips);
    }

    Err("colisse de miel".to_string())
}