use std::error::Error;
use std::io::{Read, Write};
use std::time::Duration;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

use parser_combinators::stream::ByteStream;

use crate::api::{AnyActor, AnySender, Envelope};

pub struct StartServer;

struct Loop;
struct Connect { socket: Option<TcpStream>, keep_alive: bool }

#[derive(Debug)]
struct Work { is_readable: bool, is_writable: bool }

pub struct Server {
    poll: Poll,
    events: Events,
    socket: TcpListener,
    counter: usize,
}

impl Server {
    pub fn listen(addr: &str) -> Result<Server, Box<dyn Error>> {
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(1024);
        let addr = addr.parse()?;
        let mut socket = TcpListener::bind(addr)?;
        poll.registry().register(&mut socket, Token(0), Interest::READABLE).unwrap();

        let listener = Server {
            poll,
            events,
            socket,
            counter: 0,
        };
        Ok(listener)
    }
}

impl AnyActor for Server {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(_) = envelope.message.downcast_ref::<Loop>() {
            self.poll.poll(&mut self.events, Some(Duration::from_millis(1))).unwrap();
            for event in self.events.iter() {
                match event.token() {
                    Token(0) => {
                        loop {
                            if let Ok((mut socket, _)) = self.socket.accept() {
                                self.counter += 1;
                                let token = Token(self.counter);
                                self.poll.registry()
                                    .register(&mut socket, token,
                                              Interest::READABLE | Interest::WRITABLE)
                                    .unwrap();
                                let tag = format!("{}", self.counter);
                                sender.spawn(&tag, || Box::new(Connection::default()));
                                let connect = Connect { socket: Some(socket), keep_alive: true };
                                sender.send(&tag, Envelope::of(connect));
                            } else {
                                break
                            }
                        }
                    },
                    token => {
                        let tag = format!("{}", token.0);
                        let work = Work { is_readable: event.is_readable(), is_writable: event.is_writable() };
                        sender.send(&tag, Envelope::of(work));
                    }
                }
            }
            let me = sender.myself();
            sender.send(&me, Envelope::of(Loop));
        } else if let Some(_) = envelope.message.downcast_ref::<StartServer>() {
            let me = sender.myself();
            sender.send(&me, Envelope::of(Loop));
        }
    }
}

struct Connection {
    socket: Option<TcpStream>,
    is_open: bool,
    keep_alive: bool,
    recv_buf: ByteStream,
    send_buf: ByteStream,
    can_read: bool,
    can_write: bool,
    buffer: [u8; 1024],
}

impl Connection {
    fn keep_open(&mut self, sender: &mut dyn AnySender) -> bool {
        if !self.is_open {
            if !self.keep_alive {
                self.is_open = true;
            } else {
                self.socket = None;
                let me = sender.myself();
                sender.stop(&me);
            }
        }
        self.is_open
    }
}

impl Default for Connection {
    fn default() -> Self {
        Connection {
            socket: None,
            is_open: true,
            keep_alive: true,
            recv_buf: ByteStream::with_capacity(1024),
            send_buf: ByteStream::with_capacity(1024),
            can_read: false,
            can_write: false,
            buffer: [0 as u8; 1024],
        }
    }
}

impl AnyActor for Connection {
    fn receive(&mut self, mut envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(connect) = envelope.message.downcast_mut::<Connect>() {
            self.socket = connect.socket.take();
            self.keep_alive = connect.keep_alive;
        } else if self.socket.is_none() {
            let me = sender.myself();
            sender.send(&me, envelope);
        } else if let Some(work) = envelope.message.downcast_ref::<Work>() {
            self.can_read = work.is_readable;
            self.can_write = self.can_write || work.is_writable;
            if self.can_read {
                match self.socket.as_ref().unwrap().read(&mut self.buffer[..]) {
                    Ok(0) | Err(_) => {
                        self.is_open = false;
                    },
                    Ok(n) => {
                        self.recv_buf.put(&self.buffer[0..n]);
                    }
                }
            }

            if !self.keep_open(sender) {
                return;
            }

            if self.recv_buf.len() > 0 {
                // TODO ready to read
            }

            if self.can_write && self.send_buf.len() > 0 {
                if self.send_buf.len() > 0 {
                    match self.socket.as_ref().unwrap().write_all(self.send_buf.as_ref()) {
                        Ok(_) => {
                            self.send_buf.clear();
                        },
                        _ => {
                            self.is_open = false;
                        }
                    }
                }
            }

            self.keep_open(sender);
        }
    }
}
