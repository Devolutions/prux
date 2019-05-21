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
extern crate parking_lot;
extern crate reqwest;
extern crate base64;

use hyper::Uri;
use env_logger::Builder;
use log::LevelFilter;
use std::env;
use std::net::SocketAddr;
use std::net;
use dns_lookup::lookup_host;
use tokio_tcp::TcpListener;
use tokio::prelude::*;
use tokio::io::{copy, shutdown};
use tokio_tcp::{TcpStream, ConnectFuture};

use futures::stream::*;
use futures::sink::*;
use parking_lot::Mutex;
use std::sync::Arc;
use std::io;
use std::net::Shutdown;
use crate::http::request::HttpRequest;

pub type IpResolver = Arc<HttpRequest>;

mod settings;
mod proxy;
mod http;

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

    let ip_resolver = Arc::new(HttpRequest::new(&config.server.maxmind_id, &config.server.maxmind_password));

    let server_addr = sockaddr_from_uri(config.server.uri.as_str()).unwrap();

    let addr = (net::Ipv4Addr::new(0, 0, 0, 0), config.listener.port).into();
    let listener = TcpListener::bind(&addr).unwrap();

    let done = listener.incoming()
        .map_err(|e| error!("error accepting socket; error = {:?}", e))
        .for_each(move |client| {
            let server = TcpStream::connect(&server_addr);

            let ipr = ip_resolver.clone();
            let amounts = server.and_then(move |server| {
                let client_addr = client.peer_addr();

                let client_reader = SharedStream::new(client);
                let client_writer = client_reader.clone();
                let server_reader = SharedStream::new(server);
                let server_writer = server_reader.clone();

                let shutdown_closure = |(n, _, server_writer)| {
                    shutdown(server_writer).map(move |_| n)
                };

                let client_to_server = match client_addr {
                    Ok(std::net::SocketAddr::V4(ip)) if ipv4addr_is_global(ip.ip()) => {
                        proxy::start_transmit(client_reader, server_writer, Some((ip.ip().clone(), ipr)))
                            .and_then(shutdown_closure)
                    }
                    _ => {
                        proxy::start_transmit(client_reader, server_writer, None)
                            .and_then(shutdown_closure)
                    }
                };

                let server_to_client = proxy::transmit(server_reader, client_writer)
                    .and_then(|(n, _, client_writer)| {
                        shutdown(client_writer).map(move |_| n)
                    });

                client_to_server.join(server_to_client)
            });

            let msg = amounts.map(|(_from_client, _from_server)| {})
                .map_err(|e| {
                    error!("error: {}", e);
                });

            tokio::spawn(msg);

            Ok(())
        });

    tokio::run(done);
}

#[derive(Clone)]
struct SharedStream {
    socket: std::sync::Arc<Mutex<TcpStream>>,
}

impl SharedStream {
    pub fn new(socket: TcpStream) -> Self {
        SharedStream {
            socket: std::sync::Arc::new(Mutex::new(socket))
        }
    }
}

unsafe impl std::marker::Send for SharedStream {}
unsafe impl std::marker::Sync for SharedStream {}

impl Read for SharedStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.socket.lock().read(buf)
    }
}

impl Write for SharedStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.socket.lock().write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

impl AsyncRead for SharedStream {}

impl AsyncWrite for SharedStream {
    fn shutdown(&mut self) -> Result<Async<()>, std::io::Error> {
        self.socket.lock().shutdown(Shutdown::Write)?;

        Ok(().into())
    }
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