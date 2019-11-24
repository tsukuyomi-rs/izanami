use bytes::Bytes;

#[derive(Debug)]
pub enum Message {
    Bytes(Bytes),
    String(String),
    Ping,
    Pong,
}
