#[derive(Debug)]
pub enum Error {
    ThreadPoolTooSmall { required: usize, available: usize },
    InvalidServerAddress(String),
    ServerBindFailed(u16),
}

impl Into<Box<dyn std::error::Error>> for Error {
    fn into(self) -> Box<dyn std::error::Error> {
        match self {
            Error::ThreadPoolTooSmall { required, available } =>
                format!("Not enough threads in the pool: available={} but required={}",
                        available, required).into(),
            Error::InvalidServerAddress(addr) =>
                format!("Invalid server address: {}", addr).into(),
            Error::ServerBindFailed(port) =>
                format!("Failed to bind server on port {}", port).into(),
        }
    }
}
