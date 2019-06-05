pub mod protocol;
pub mod injector;
pub mod stream_collection;

use std::io;

use futures::{Future, Poll};
use std::net::Ipv4Addr;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::prelude::*;
use crate::proxy::protocol::Protocol;
use crate::IpResolver;
use crate::proxy::protocol::read_proto;

fn find_bytes_pos(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle).and_then(|pos| Some(pos + needle.len()))
}

pub fn start_transmit<R, W>(reader: R, writer: W, detect: Option<(Ipv4Addr, IpResolver)>) -> impl Future<Item=(u64, R, W), Error=io::Error> + Send
    where R: AsyncRead + Send + 'static,
          W: AsyncWrite + Send + 'static, {
    read_proto(reader, detect.is_none()).and_then(move |(proto, reader, pos, cap, amt, buf)| {
        if let (Protocol::Http11(_, _), Some(ipr)) = (&proto, detect) {
            Box::new(injector::inject_basic_hdr(ipr).map_err(|_| io::Error::new(io::ErrorKind::WriteZero, "write zero byte into writer")).and_then(|vec: Vec<u8>| {
                let buf_head_cap = find_bytes_pos(&buf, crate::proxy::injector::HDR_SEP).unwrap();
                let buf_cap = buf.len();
                let hdrs_cap = vec.len();
                Injector {
                    reader: Some(reader),
                    writer: Some(writer),
                    buf_pos: 0,
                    buf_head_cap,
                    buf_cap,
                    buf: Some(buf),
                    hdrs_pos: 0,
                    hdrs_cap,
                    hdrs: Some(vec),
                }
            }).and_then(move |(reader, writer, buf)| {
                Transmit {
                    proto: Some(proto),
                    reader: Some(reader),
                    read_done: false,
                    writer: Some(writer),
                    pos: 0,
                    cap: 0,
                    amt: 0,
                    buf,
                }
            })) as Box<Future<Item=(u64, R, W), Error=io::Error> + Send>
        } else {
            Box::new(Transmit {
                proto: Some(proto),
                reader: Some(reader),
                read_done: false,
                writer: Some(writer),
                amt,
                pos,
                cap,
                buf,
            }) as Box<Future<Item=(u64, R, W), Error=io::Error> + Send>
        }
    })
}

#[derive(Debug)]
pub struct Injector<R, W> {
    reader: Option<R>,
    writer: Option<W>,
    buf_pos: usize,
    buf_head_cap: usize,
    buf_cap: usize,
    buf: Option<Box<[u8]>>,
    hdrs_pos: usize,
    hdrs_cap: usize,
    hdrs: Option<Vec<u8>>,
}

impl<R, W> Future for Injector<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    type Item = (R, W, Box<[u8]>);
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {

        // write the head of the http 1.1 req
        while self.buf_pos < self.buf_head_cap {
            let writer = self.writer.as_mut().unwrap();
            let buf = self.buf.as_mut().unwrap();
            let writer = self.writer.as_mut().unwrap();
            match writer.poll_write(&buf[self.buf_pos..self.buf_head_cap]) {
                Ok(Async::Ready(i)) => {
                    if i == 0 {
                        return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                  "write zero byte into writer"));
                    } else {
                        self.buf_pos += i;
                    }
                }
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady);
                }
                Err(e) => return Err(e),
            }
        }

        // write injected hdrs
        while self.hdrs_pos < self.hdrs_cap {
            let writer = self.writer.as_mut().unwrap();
            let hdrs_buf = self.hdrs.as_mut().unwrap();
            match writer.poll_write(&hdrs_buf[self.hdrs_pos..self.hdrs_cap]) {
                Ok(Async::Ready(i)) => {
                    if i == 0 {
                        return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                  "write zero byte into writer"));
                    } else {
                        self.hdrs_pos += i;
                    }
                }
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady);
                }
                Err(e) => return Err(e),
            }
        }

        // write the rest
        while self.buf_pos < self.buf_cap {
            let writer = self.writer.as_mut().unwrap();
            let buf = self.buf.as_mut().unwrap();
            match writer.poll_write(&buf[self.buf_pos..self.buf_cap]) {
                Ok(Async::Ready(i)) => {
                    if i == 0 {
                        return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                  "write zero byte into writer"));
                    } else {
                        self.buf_pos += i;
                    }
                }
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady);
                }
                Err(e) => return Err(e),
            }
        }

        // If we've written all the data and we've seen EOF, flush out the
        // data and finish the transfer.
        // done with the entire transfer.
        match self.writer.as_mut().unwrap().poll_flush() {
            Ok(Async::Ready(())) => {}
            Ok(Async::NotReady) => {
                return Ok(Async::NotReady);
            }
            Err(e) => return Err(e),
        }

        let reader = self.reader.take().unwrap();
        let writer = self.writer.take().unwrap();
        let buf = self.buf.take().unwrap();
        Ok((reader, writer, buf).into())
    }
}

#[derive(Debug)]
pub struct Transmit<R, W> {
    proto: Option<Protocol>,
    reader: Option<R>,
    read_done: bool,
    writer: Option<W>,
    pos: usize,
    cap: usize,
    amt: u64,
    buf: Box<[u8]>,
}

pub fn transmit<R, W>(reader: R, writer: W) -> Transmit<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    Transmit {
        proto: None,
        reader: Some(reader),
        read_done: false,
        writer: Some(writer),
        amt: 0,
        pos: 0,
        cap: 0,
        buf: Box::new([0u8; 2048]),
    }
}

impl<R, W> Future for Transmit<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    type Item = (u64, R, W);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(u64, R, W), tokio::io::Error> {
        loop {
            // If our buffer is empty, then we need to read some data to
            // continue.
            if self.pos == self.cap && !self.read_done {
                let reader = self.reader.as_mut().unwrap();
                match reader.poll_read(&mut self.buf) {
                    Ok(Async::Ready(n)) => {
                        if n == 0 {
                            self.read_done = true;
                        } else {
                            self.pos = 0;
                            self.cap = n;
                        }
                    }
                    Ok(Async::NotReady) => {
                        return Ok(Async::NotReady);
                    }
                    Err(e) => return Err(e),
                }
            }

            // If our buffer has some data, let's write it out!
            while self.pos < self.cap {
                let writer = self.writer.as_mut().unwrap();
                match writer.poll_write(&self.buf[self.pos..self.cap]) {
                    Ok(Async::Ready(i)) => {
                        if i == 0 {
                            return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                      "write zero byte into writer"));
                        } else {
                            self.pos += i;
                            self.amt += i as u64;
                        }
                    }
                    Ok(Async::NotReady) => {
                        return Ok(Async::NotReady);
                    }
                    Err(e) => return Err(e),
                }
            }

            // If we've written al the data and we've seen EOF, flush out the
            // data and finish the transfer.
            // done with the entire transfer.
            if self.pos == self.cap && self.read_done {
                match self.writer.as_mut().unwrap().poll_flush() {
                    Ok(Async::Ready(())) => {}
                    Ok(Async::NotReady) => {
                        return Ok(Async::NotReady);
                    }
                    Err(e) => return Err(e),
                }
                let reader = self.reader.take().unwrap();
                let writer = self.writer.take().unwrap();
                return Ok((self.amt, reader, writer).into());
            }
        }
    }
}
