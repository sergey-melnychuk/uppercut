use std::error::Error;
use std::net::SocketAddr;
use std::io::{Read, Write, ErrorKind};
use std::time::Duration;

use mio::{Poll, Events, Token, Interest};
use mio::net::TcpStream;

use parser_combinators::stream::ByteStream;
use std::collections::HashMap;


// TODO make poll timeout, buffer sizes, pooling, etc configurable by introducing ClientConfig

pub struct Client {
    poll: Poll,
    events: Events,
    counter: usize,
    connections: HashMap<usize, Connection>,
    destinations: HashMap<String, usize>,
}

impl Client {
    pub fn new() -> Self {
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(1024);

        Client {
            poll,
            events,
            counter: 0,
            connections: HashMap::new(),
            destinations: HashMap::new(),
        }
    }

    pub fn connect(&mut self, target: &str) -> Result<usize, Box<dyn Error>> {
        self.counter += 1;

        let addr: SocketAddr = target.parse()?;
        let mut socket = TcpStream::connect(addr)?;
        let id = self.counter;
        self.poll.registry().register(&mut socket, Token(id), Interest::WRITABLE).unwrap();

        let connection = Connection::connected(id, addr.to_string(), socket, 1024);
        self.connections.insert(id, connection);
        self.destinations.insert(addr.to_string(), id);
        Ok(id)
    }

    pub fn poll(&mut self, timeout: Option<Duration>) {
        self.poll.poll(&mut self.events, timeout).unwrap();

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
                                Interest::READABLE.add(Interest::READABLE)).unwrap();

                self.connections.insert(id, connection);
            } else {
                self.destinations.remove(&connection.target);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    pub fn has(&self, addr: &str) -> bool {
        self.destinations.contains_key(addr)
    }

    pub fn put(&mut self, addr: &str, payload: &[u8]) -> usize {
        let target = addr.to_string();
        let id = self.destinations.get(&target).unwrap();
        self.connections.get_mut(id).unwrap().send_buf.put(payload)
    }

    pub fn get(&mut self, addr: &str) -> Option<&mut ByteStream> {
        let target = addr.to_string();
        let id = self.destinations.get(&target).unwrap();
        self.connections.get_mut(id).map(|c| &mut c.recv_buf)
    }
}

struct Connection {
    token: Token,
    target: String,
    socket: Option<TcpStream>,
    is_open: bool,
    recv_buf: ByteStream,
    send_buf: ByteStream,
    buffer: [u8; 1024],
}

impl Connection {
    fn connected(id: usize, target: String, socket: TcpStream, buffer_size: usize) -> Self {
        Self {
            token: Token(id),
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
