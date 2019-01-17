pub mod protocol;

use std::io;

use futures::{Future, Poll};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::prelude::*;
use crate::proxy::protocol::Protocol;

pub struct ProtoReader<R> where R: AsyncRead {
    reader: Option<R>,
    read_done: bool,
    pos: usize,
    cap: usize,
    amt: u64,
    buf: Option<Box<[u8]>>,
}

pub fn read_proto<R>(reader: R) -> ProtoReader<R> where R: AsyncRead {
    ProtoReader {
        reader: Some(reader),
        read_done: false,
        pos: 0,
        cap: 0,
        amt: 0,
        buf: Some(Box::new([0u8; 2048])),
    }
}

impl<R> Future for ProtoReader<R> where R: AsyncRead {
    type Item = (Protocol, R, usize, usize, u64, Box<[u8]>);
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        if self.pos == self.cap && !self.read_done {
            let reader = self.reader.as_mut().unwrap();
            match reader.poll_read(self.buf.as_mut().unwrap()) {
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

        let protocol = Protocol::detect(self.buf.as_mut().unwrap());

        return Ok((protocol, self.reader.take().unwrap(), self.pos, self.cap, self.amt, self.buf.take().unwrap()).into());
    }
}

pub fn detect_and_transmit<R, W>(reader: R, writer: W) -> futures::future::AndThen<ProtoReader<R>, Trasmit<R, W>, impl FnOnce((Protocol, R, usize, usize, u64, Box<[u8]>)) -> Trasmit<R, W>>
    where R: AsyncRead,
          W: AsyncWrite, {
    read_proto(reader).and_then(move |(proto, reader, pos, cap, amt, buf)| {
        info!("{:?}", proto);
        Trasmit {
            proto: Some(proto),
            reader: Some(reader),
            read_done: false,
            writer: Some(writer),
            amt,
            pos,
            cap,
            buf,
        }
    })
}

#[derive(Debug)]
pub struct Trasmit<R, W> {
    proto: Option<Protocol>,
    reader: Option<R>,
    read_done: bool,
    writer: Option<W>,
    pos: usize,
    cap: usize,
    amt: u64,
    buf: Box<[u8]>,
}

pub fn transmit<R, W>(reader: R, writer: W) -> Trasmit<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    Trasmit {
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

impl<R, W> Future for Trasmit<R, W>
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
