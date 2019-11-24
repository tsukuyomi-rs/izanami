use bytes::Bytes;

#[derive(Debug)]
pub enum Message {
    Text(String),
    Binary(Bytes),
    Ping(Bytes),
    Pong(Bytes),
}
