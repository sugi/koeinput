pub mod file;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("{0}")]
    Io(String),
    #[error("unsupported audio: {0}")]
    Format(String),
    #[error("audio device: {0}")]
    Device(String),
}
