//! A minimal MQTT 3.1.1 client: QoS 0 publish, a retained last will, keepalive.
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Options<'a> {
    pub host: &'a str,
    pub port: u16,
    pub client_id: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    /// Topic and payload the broker publishes, retained, when the connection drops.
    pub will: (&'a str, &'a str),
    pub keepalive: u16,
}

pub struct Client {
    stream: TcpStream,
    buf: Vec<u8>,
    keepalive: Duration,
    last_sent: Instant,
    last_heard: Instant,
}

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s);
}

fn packet(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    let mut n = body.len();
    loop {
        let b = (n % 128) as u8;
        n /= 128;
        out.push(if n > 0 { b | 128 } else { b });
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    out
}

/// The first complete packet in `buf`: its type byte, body and total length.
fn split(buf: &[u8]) -> Option<(u8, &[u8], usize)> {
    let (mut len, mut shift, mut i) = (0usize, 0, 1);
    loop {
        let b = *buf.get(i)?;
        len |= ((b & 127) as usize) << shift;
        i += 1;
        if b & 128 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return None;
        }
    }
    let end = i + len;
    (buf.len() >= end).then(|| (buf[0], &buf[i..end], end))
}

/// Topic and payload of a PUBLISH body.
#[cfg(test)]
fn publish_parts(kind: u8, body: &[u8]) -> Option<(String, Vec<u8>)> {
    let len = u16::from_be_bytes([*body.first()?, *body.get(1)?]) as usize;
    let topic = std::str::from_utf8(body.get(2..2 + len)?).ok()?.to_string();
    let start = 2 + len + if (kind >> 1) & 3 > 0 { 2 } else { 0 };
    Some((topic, body.get(start..)?.to_vec()))
}

impl Client {
    pub fn connect(o: &Options) -> io::Result<Client> {
        let addr = (o.host, o.port).to_socket_addrs()?.next().ok_or_else(|| io::Error::other("no address"))?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut flags = 0x02 | 0x04 | 0x20;
        if !o.username.is_empty() {
            flags |= 0x80;
        }
        if !o.password.is_empty() {
            flags |= 0x40;
        }
        let mut body = Vec::new();
        put_str(&mut body, b"MQTT");
        body.extend_from_slice(&[4, flags]);
        body.extend_from_slice(&o.keepalive.to_be_bytes());
        put_str(&mut body, o.client_id.as_bytes());
        put_str(&mut body, o.will.0.as_bytes());
        put_str(&mut body, o.will.1.as_bytes());
        for s in [o.username, o.password] {
            if !s.is_empty() {
                put_str(&mut body, s.as_bytes());
            }
        }
        let now = Instant::now();
        let mut c = Client { stream, buf: Vec::new(), keepalive: Duration::from_secs(o.keepalive as u64), last_sent: now, last_heard: now };
        c.send(&packet(0x10, &body))?;
        while now.elapsed() < Duration::from_secs(10) {
            if let Some((kind, body)) = c.next()? {
                if kind == 0x20 {
                    return match body.get(1) {
                        Some(0) => Ok(c),
                        code => Err(io::Error::other(format!("connection refused: {code:?}"))),
                    };
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "no CONNACK"))
    }

    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        self.stream.write_all(data)?;
        self.last_sent = Instant::now();
        Ok(())
    }

    pub fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> io::Result<()> {
        let mut body = Vec::new();
        put_str(&mut body, topic.as_bytes());
        body.extend_from_slice(payload);
        self.send(&packet(0x30 | retain as u8, &body))
    }

    /// The next packet, waiting up to a second for data.
    fn next(&mut self) -> io::Result<Option<(u8, Vec<u8>)>> {
        if let Some((kind, body, end)) = split(&self.buf) {
            let p = (kind, body.to_vec());
            self.buf.drain(..end);
            return Ok(Some(p));
        }
        let mut chunk = [0u8; 2048];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "broker closed the connection")),
            Ok(n) => {
                self.last_heard = Instant::now();
                self.buf.extend_from_slice(&chunk[..n]);
                Ok(None)
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Waits up to about a second for the broker, keeping the connection alive.
    pub fn poll(&mut self) -> io::Result<()> {
        self.next()?;
        if self.last_heard.elapsed() > self.keepalive * 3 / 2 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "broker stopped answering"));
        }
        if self.last_sent.elapsed() >= self.keepalive / 2 {
            self.send(&[0xc0, 0])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut body = Vec::new();
        put_str(&mut body, b"open-cgs2/state");
        body.extend_from_slice(&[b'x'; 300]);
        let p = packet(0x30, &body);
        assert_eq!(&p[1..3], &[0xbd, 0x02]);
        let (kind, b, end) = split(&p).unwrap();
        assert_eq!(end, p.len());
        assert_eq!(publish_parts(kind, b), Some(("open-cgs2/state".into(), vec![b'x'; 300])));
        assert!(split(&p[..p.len() - 1]).is_none());
    }
}
