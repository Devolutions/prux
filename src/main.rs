extern crate tokio;
extern crate tokio_io;
extern crate tokio_tcp;
extern crate futures;
#[macro_use]
extern crate serde_derive;
#[macro_use]
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

use hyper::Uri;

use env_logger::Builder;
use log::LevelFilter;
use std::env;
use std::net::SocketAddr;
use std::net;
use dns_lookup::lookup_host;
use tokio_tcp::TcpListener;
use tokio::prelude::*;

mod settings;
mod proxy;

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

    let addr = (net::Ipv4Addr::new(0,0,0,0), config.listener.port).into();
    let listener = TcpListener::bind(&addr).unwrap();

    // accept connections and process them
    tokio::run(listener.incoming()
        .map_err(|e| error!("failed to accept socket; error = {:?}", e))
        .for_each(|socket| {
            info!("Peer connected from {:?}", socket.peer_addr());
            Ok(())
        })
    );
}

pub fn sockaddr_from_uri(uri: &str) -> Result<SocketAddr, String> {
    let uri: Uri = uri.parse().map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;
    let port;

    let ip = {
        if let Ok(addrs) = get_addr_from_uri(&uri) {
            addrs[0]
        } else {
            return Err(String::from(
                "No local ipAddr specified"));
        }
    };

    if let Some(p) = uri.port() {
        port = p
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