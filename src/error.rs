#[derive(Debug)]
pub enum Error {
    ThreadPoolTooSmall { required: usize, available: usize },
    InvalidServerAddress(String),
    ServerBindFailed(u16),
}

impl From<Error> for Box<dyn std::error::Error> {
    fn from(error: Error) -> Box<dyn std::error::Error> {
        match error {
            Error::ThreadPoolTooSmall {
                required,
                available,
            } => format!(
                "Not enough threads in the pool: available={} but required={}",
                available, required
            )
            .into(),
            Error::InvalidServerAddress(addr) => format!("Invalid server address: {}", addr).into(),
            Error::ServerBindFailed(port) => {
                format!("Failed to bind server on port {}", port).into()
            }
        }
    }
}
