use bytes::{Bytes, BytesMut, BufMut};

use std::mem::size_of;
use parsed::stream::ByteStream;

pub struct Packet {
    pub to: String,
    pub from: String,
    pub payload: Vec<u8>,
    pub port: u16,
}

impl Packet {
    pub fn new(to: String, from: String, payload: Vec<u8>, port: u16) -> Self {
        Self {
            to,
            from,
            payload,
            port,
        }
    }

    pub fn to_bytes(&self) -> Bytes {
        let cap: usize = 3 * size_of::<u32>() + size_of::<u16>()
            + self.payload.len() + self.to.len() + self.from.len();
        let mut bytes = BytesMut::with_capacity(cap);
        bytes.put_u32(self.to.len() as u32);
        bytes.put_u32(self.from.len() as u32);
        bytes.put_u32(self.payload.len() as u32);
        bytes.put_u16(self.port);
        bytes.put(self.to.as_bytes());
        bytes.put(self.from.as_bytes());
        bytes.put(self.payload.as_ref());
        bytes.freeze()
    }

    pub fn from_bytes(stream: &mut ByteStream, limit: usize) -> Result<Option<Packet>, ()> {
        let min = 3 * size_of::<u32>() + size_of::<u16>();
        if stream.len() <= min {
            return Ok(None);
        }
        let (to_len, from_len, payload_len, port) = (
            stream.get_u32().unwrap() as usize,
            stream.get_u32().unwrap() as usize,
            stream.get_u32().unwrap() as usize,
            stream.get_u16().unwrap()
        );
        let total = min + to_len + from_len + payload_len;
        if total > limit {
            // Naive check if such stream will ever match into a Packet
            return Err(());
        }
        if stream.len() < total {
            return Ok(None);
        }

        let to = String::from_utf8(stream.get(to_len).unwrap()).unwrap();
        let from = String::from_utf8(stream.get(from_len).unwrap()).unwrap();
        let payload = stream.get(payload_len).unwrap();
        stream.pull();

        Ok(Some(Packet {
            to,
            from,
            payload,
            port
        }))
    }
}
