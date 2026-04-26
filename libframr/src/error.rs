use std::fmt;

#[derive(Debug)]
pub enum FramrError {
    ConnectionFailed(String),
    NoOutputs,
    OutputNotFound(usize),
    NoSupportedBufferFormat,
    FrameCaptureFailed,
    ProtocolNotSupported(String),
    Io(std::io::Error),
}

impl fmt::Display for FramrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::NoOutputs => write!(f, "no outputs found"),
            Self::OutputNotFound(idx) => write!(f, "output {idx} not found"),
            Self::NoSupportedBufferFormat => write!(f, "no supported buffer format"),
            Self::FrameCaptureFailed => write!(f, "frame capture failed"),
            Self::ProtocolNotSupported(name) => write!(f, "protocol not supported: {name}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for FramrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FramrError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
