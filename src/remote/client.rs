use std::error::Error;
use std::net::SocketAddr;
use std::io::{Read, Write, ErrorKind};
use std::time::Duration;

use mio::{Poll, Events, Token, Interest};
use mio::net::TcpStream;

use parsed::stream::ByteStream;
use std::collections::HashMap;

use bytes::{BytesMut, BufMut};

use crate::api::{AnyActor, AnySender, Envelope};


pub struct Client {
    poll: Poll,
    events: Events,
    counter: usize,
    connections: HashMap<usize, Connection>,
    destinations: HashMap<String, usize>,
    response_port: u16,
}

impl Client {
    pub fn new(response_port: u16) -> Self {
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(1024);

        Client {
            poll,
            events,
            counter: 0,
            connections: HashMap::new(),
            destinations: HashMap::new(),
            response_port,
        }
    }

    pub fn connect(&mut self, target: &str) -> Result<usize, Box<dyn Error>> {
        self.counter += 1;

        let addr: SocketAddr = target.parse()?;
        let mut socket = TcpStream::connect(addr)?;
        let id = self.counter;
        self.poll.registry().register(&mut socket, Token(id), Interest::WRITABLE).unwrap();

        let connection = Connection::connected(addr.to_string(), socket, 1024);
        self.connections.insert(id, connection);
        self.destinations.insert(addr.to_string(), id);
        Ok(id)
    }

    pub fn poll(&mut self, timeout: Duration) {
        self.poll.poll(&mut self.events, Some(timeout)).unwrap();

        for event in &self.events {
            let id = event.token().0;

            let mut connection = self.connections.remove(&id).unwrap();

            if event.is_readable() {
                let _ = connection.recv();
                connection.is_open = !event.is_read_closed();
            }

            if event.is_writable() {
                connection.send();
                connection.send_buf.pull();
                connection.is_open = !event.is_write_closed();
            }

            if connection.is_open {
                self.poll.registry()
                    .reregister(connection.socket.as_mut().unwrap(),
                                event.token(),
                                Interest::READABLE.add(Interest::WRITABLE)).unwrap();

                self.connections.insert(id, connection);
            } else {
                self.destinations.remove(&connection.target);
            }
        }
    }

    pub fn has(&self, addr: &str) -> bool {
        self.destinations.contains_key(addr)
    }

    pub fn put(&mut self, addr: &str, payload: &[u8]) -> usize {
        let target = addr.to_string();
        let id = self.destinations.get(&target).unwrap();
        self.connections.get_mut(id).unwrap().send_buf.put(payload)
    }
}

struct Connection {
    target: String,
    socket: Option<TcpStream>,
    is_open: bool,
    recv_buf: ByteStream,
    send_buf: ByteStream,
    buffer: [u8; 1024],
}

impl Connection {
    fn connected(target: String, socket: TcpStream, buffer_size: usize) -> Self {
        Self {
            target,
            socket: Some(socket),
            is_open: true,
            recv_buf: ByteStream::with_capacity(buffer_size),
            send_buf: ByteStream::with_capacity(buffer_size),
            buffer: [0u8; 1024],
        }
    }
}

impl Connection {
    fn send(&mut self) {
        match self.socket.as_ref().unwrap().write_all(self.send_buf.as_ref()) {
            Ok(_) => {
                self.send_buf.clear();
            },
            Err(_) => {
                self.is_open = false;
            }
        }
    }

    fn recv(&mut self) -> usize {
        let mut bytes_received: usize = 0;
        while self.recv_buf.cap() >= self.buffer.len() {
            match self.socket.as_ref().unwrap().read(&mut self.buffer) {
                Ok(n) if n > 0 => {
                    self.recv_buf.put(&self.buffer[0..n]);
                    bytes_received += n;
                },
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Ok(_) | Err(_) => {
                    self.is_open = false;
                    break
                }
            }
        }
        bytes_received
    }
}

#[derive(Debug)]
pub struct StartClient;

#[derive(Debug)]
struct Loop;

impl AnyActor for Client {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if envelope.message.downcast_ref::<Loop>().is_some() {
            self.poll(Duration::from_millis(1));
            let me = sender.myself();
            sender.send(&me, envelope);
        } else if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {

            // TODO extract preparation of payload for sending into the socket
            let (to, host) = split_address(envelope.to);
            let from = envelope.from.to_owned();
            let ok = self.has(&host) || self.connect(&host).is_ok();

            if ok {
                // TODO extract binary serialization logic
                let cap: usize = 3 * 4 + 2 + vec.len() + to.len() + from.len();
                let mut buf = BytesMut::with_capacity(cap);
                buf.put_u32(to.len() as u32);
                buf.put_u32(from.len() as u32);
                buf.put_u32(vec.len() as u32);
                buf.put_u16(self.response_port);
                buf.put(to.as_bytes());
                buf.put(from.as_bytes());
                buf.put(vec.as_ref());

                sender.log(&format!("client/sent: to={}[@{}] from={} vec={:?}", to, host, from, vec));
                self.put(&host, buf.as_ref());
            }

        } else if envelope.message.downcast_ref::<StartClient>().is_some() {
            let me = sender.myself();
            sender.send(&me, Envelope::of(Loop));
        }
    }
}

fn split_address(address: String) -> (String, String) {
    if address.contains('@') {
        let split = address.split('@')
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        (split[0].to_owned(), split[1].to_owned())
    } else {
        (address, String::default())
    }
}
