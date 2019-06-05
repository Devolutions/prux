use tokio::net::tcp::ConnectFuture;
use std::net::SocketAddr;
use std::collections::VecDeque;
use tokio_tcp::TcpStream;
use futures::{Stream, future};
use tokio::prelude::Async;
use futures::future::Future;
use futures::sink::Sink;
use crossbeam_channel::{Sender, TrySendError};
use std::error::Error;

pub const MAX: usize = 256;

pub struct Streams {
    tx: Sender<TcpStream>,
    conn_future: Option<ConnectFuture>,
    conn_stream: Option<TcpStream>,
    server_addr: SocketAddr,
}

impl Streams {
    pub fn new(server_addr: SocketAddr, tx: Sender<TcpStream>) -> Self {
        let conn_future = Some(TcpStream::connect(&server_addr));
        Streams {
            tx,
            conn_future,
            conn_stream: None,
            server_addr,
        }
    }
}

impl Stream for Streams {
    type Item = ();
    type Error = String;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        if let Some(stream) = self.conn_stream.take() {
            match self.tx.try_send(stream) {
                Ok(()) => {
                    self.conn_future = Some(TcpStream::connect(&self.server_addr));
                }

                Err(e) => {
                    match e {
                        TrySendError::Full(s) => {
                            self.conn_stream = Some(s);
                            return Ok(Async::Ready(Some(())))
                        },

                        TrySendError::Disconnected(_) => {
                            return Err("Disconnected channel".to_string())
                        }
                    }
                }
            }
        }

        if let Some(fut) = self.conn_future.as_mut().take() {
            match fut.poll() {
                Ok(Async::Ready(stream)) => {
                    self.conn_stream = Some(stream)
                },

                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady)
                },

                Err(e) => {
                    return Err(format!("ioerror: {}", e))
                },
            }
        }

        Ok(Async::Ready(Some(())))
    }
}