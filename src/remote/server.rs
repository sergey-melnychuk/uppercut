use std::io::{Read, Write};
use std::net::SocketAddr;
use std::time::Duration;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

use parsed::stream::ByteStream;

use crate::api::{AnyActor, AnySender, Envelope};
use crate::config::ServerConfig;
use crate::error::Error;
use crate::remote::packet::Packet;

#[derive(Debug)]
pub(crate) struct Loop;

#[derive(Debug)]
struct Work {
    is_readable: bool,
    is_writable: bool,
}

pub struct Server {
    config: ServerConfig,
    poll: Poll,
    events: Events,
    socket: TcpListener,
    counter: usize,
    port: u16,
}

// TODO Make configurable?
const PACKET_SIZE_LIMIT: usize = 4096 + 12; // 12 bytes header + max 4kb payload

impl Server {
    pub fn listen(addr: &str, config: &ServerConfig) -> Result<Server, Error> {
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(config.events_capacity);
        let addr = addr
            .parse::<SocketAddr>()
            .map_err(|_| Error::InvalidServerAddress(addr.to_string()))?;
        let port = addr.port();
        let mut socket = TcpListener::bind(addr).map_err(|_| Error::ServerBindFailed(port))?;
        poll.registry()
            .register(&mut socket, Token(0), Interest::READABLE)
            .unwrap();

        let listener = Server {
            config: config.clone(),
            poll,
            events,
            socket,
            counter: 0,
            port,
        };
        Ok(listener)
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Server {
    fn tag(me: &str, id: usize) -> String {
        format!("{}#{:012}", me, id)
    }
}

impl AnyActor for Server {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if envelope.message.downcast_ref::<Loop>().is_some() {
            self.poll
                .poll(&mut self.events, Some(Duration::from_millis(1)))
                .unwrap();
            for event in self.events.iter() {
                match event.token() {
                    Token(0) => {
                        while let Ok((mut socket, _remote)) = self.socket.accept() {
                            self.counter += 1;
                            let token = Token(self.counter);
                            self.poll
                                .registry()
                                .register(
                                    &mut socket,
                                    token,
                                    Interest::READABLE | Interest::WRITABLE,
                                )
                                .unwrap();
                            let connection = Connection {
                                socket,
                                keep_alive: true,
                                recv_buf: ByteStream::with_capacity(self.config.recv_buffer_size),
                                send_buf: ByteStream::with_capacity(self.config.send_buffer_size),
                                is_open: true,
                                can_read: false,
                                can_write: false,
                            };
                            let tag = Server::tag(sender.me(), self.counter);
                            sender.spawn(&tag, Box::new(move || Box::new(connection)));
                        }
                    }
                    token => {
                        let tag = Server::tag(sender.me(), token.0);
                        let work = Work {
                            is_readable: event.is_readable(),
                            is_writable: event.is_writable(),
                        };
                        sender.send(&tag, Envelope::of(work));
                    }
                }
            }
            sender.send(sender.me(), Envelope::of(Loop));
        }
    }
}

struct Connection {
    socket: TcpStream,
    is_open: bool,
    keep_alive: bool,
    recv_buf: ByteStream,
    send_buf: ByteStream,
    can_read: bool,
    can_write: bool,
}

impl Connection {
    fn keep_open(&mut self, sender: &mut dyn AnySender) -> bool {
        if !self.is_open {
            if !self.keep_alive {
                self.is_open = true;
            } else {
                sender.stop(sender.me());
            }
        }
        self.is_open
    }
}

impl AnyActor for Connection {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(work) = envelope.message.downcast_ref::<Work>() {
            let mut buffer = [0u8; 1024];
            self.can_read = work.is_readable;
            self.can_write = self.can_write || work.is_writable;
            if self.can_read {
                match self.socket.read(&mut buffer[..]) {
                    Ok(0) | Err(_) => {
                        self.is_open = false;
                    }
                    Ok(n) => {
                        self.recv_buf.put(&buffer[0..n]);
                    }
                }
            }

            if !self.keep_open(sender) {
                return;
            }

            loop {
                let r = Packet::from_bytes(&mut self.recv_buf, PACKET_SIZE_LIMIT);
                if r.is_err() {
                    sender.log("Packet parser marked connection buffer as failed");
                    self.is_open = false;
                    break;
                }

                if let Ok(Some(packet)) = r {
                    let mut host = self.socket.peer_addr().unwrap();
                    host.set_port(packet.port);
                    let from = format!("{}@{}", packet.from, host);

                    sender.log(&format!(
                        "server/rcvd: to={} from={} bytes={}",
                        packet.to,
                        packet.from,
                        packet.payload.len()
                    ));
                    let e = Envelope::of(packet.payload).to(&packet.to).from(&from);
                    sender.send(&packet.to, e);
                } else {
                    break;
                }
            }

            if self.can_write && self.send_buf.len() > 0 {
                match self.socket.write_all(self.send_buf.as_ref()) {
                    Ok(_) => {
                        self.send_buf.clear();
                    }
                    _ => {
                        self.is_open = false;
                    }
                }
            }

            self.keep_open(sender);
        }
    }
}
