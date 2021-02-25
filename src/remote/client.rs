use std::error::Error;
use std::net::SocketAddr;
use std::io::{Read, Write, ErrorKind};
use std::time::Duration;
use std::collections::HashMap;

use mio::{Poll, Events, Token, Interest};
use mio::net::TcpStream;

use parsed::stream::ByteStream;

use crate::api::{AnyActor, AnySender, Envelope};
use crate::config::ClientConfig;
use crate::remote::packet::Packet;


pub struct Client {
    config: ClientConfig,
    poll: Poll,
    events: Events,
    counter: usize,
    connections: HashMap<usize, Connection>,
    destinations: HashMap<String, usize>,
    response_port: u16,
}

impl Client {
    pub fn new(response_port: u16, config: &ClientConfig) -> Self {
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(config.events_capacity);

        Client {
            config: config.clone(),
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

        let connection = Connection::connected(addr.to_string(), socket, self.config.clone());
        self.connections.insert(id, connection);
        self.destinations.insert(addr.to_string(), id);
        Ok(id)
    }

    pub fn poll(&mut self, timeout: Duration) {
        self.poll.poll(&mut self.events, Some(timeout)).unwrap();

        for event in &self.events {
            let id = event.token().0;

            // TODO Extract IO to worker actors (like in server)
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
}

impl Connection {
    fn connected(target: String, socket: TcpStream, config: ClientConfig) -> Self {
        Self {
            target,
            socket: Some(socket),
            is_open: true,
            recv_buf: ByteStream::with_capacity(config.recv_buffer_size),
            send_buf: ByteStream::with_capacity(config.send_buffer_size),
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
        let mut buffer = [0u8; 1024];
        while self.recv_buf.cap() >= buffer.len() {
            match self.socket.as_ref().unwrap().read(&mut buffer) {
                Ok(n) if n > 0 => {
                    self.recv_buf.put(&buffer[0..n]);
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
        } else if let Some(payload) = envelope.message.downcast_ref::<Vec<u8>>() {
            let (to, host) = split_address(envelope.to);
            let from = envelope.from.to_owned();

            let ok = self.has(&host) || self.connect(&host).is_ok();
            if ok {
                let packet = Packet::new(to, from, payload.to_vec(), self.response_port);
                self.put(&host, packet.to_bytes().as_ref());
                sender.log(&format!("sent: to={}[@{}] from={} bytes={}",
                                    packet.to, host, packet.from, payload.len()));
            } else {
                sender.log(&format!("Failed to send: to={}[@{}] from={} bytes={}",
                                    to, host, from, payload.len()));
                // TODO Introduce some generic way to report that packet was dropped (not sent)
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
