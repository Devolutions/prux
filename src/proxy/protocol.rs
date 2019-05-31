use httparse;
use tokio::prelude::*;
use tokio::io;

pub struct ProtoReader<R> where R: AsyncRead {
    reader: Option<R>,
    read_done: bool,
    pos: usize,
    cap: usize,
    amt: u64,
    buf: Option<Box<[u8]>>,
}

pub fn read_proto<R>(reader: R, skip_read: bool) -> ProtoReader<R> where R: AsyncRead {
    ProtoReader {
        reader: Some(reader),
        read_done: skip_read,
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

#[derive(Debug)]
pub enum Protocol {
    Tcp,
    Http11(String, String), // Ain't nobody got time for HTTP/1.0
    Http2,
}

const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

impl Protocol {
    /// Tries to detect a known protocol in the peeked bytes.
    ///
    /// If no protocol can be determined, returns `Protocol::Tcp`.
    pub fn detect(bytes: &[u8]) -> Protocol {
        // http2 is easiest to detect
        if bytes.len() >= H2_PREFACE.len() {
            if &bytes[..H2_PREFACE.len()] == H2_PREFACE {
                return Protocol::Http2;
            }
        }

        // http1 can have a really long first line, but if the bytes so far
        // look like http1, we'll assume it is. a different protocol
        // should look different in the first few bytes

        let mut headers = [httparse::EMPTY_HEADER; 0];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(bytes) {
            // Ok(Complete) or Ok(Partial) both mean it looks like HTTP1!
            //
            // If we got past the first line, we'll see TooManyHeaders,
            // because we passed an array of 0 headers to parse into. That's fine!
            // We didn't want to keep parsing headers, just validate that
            // the first line is HTTP1.
            Ok(_) | Err(httparse::Error::TooManyHeaders) => {
                return Protocol::Http11(req.method.unwrap_or("Unknown Method").to_string(), req.path.unwrap_or("Missing Path").to_string());
            },
            _ => {}
        }

        Protocol::Tcp
    }
}
