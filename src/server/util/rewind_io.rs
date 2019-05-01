use bytes::{Buf, BufMut, Bytes, IntoBuf};
use futures::{Async, Poll};
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug)]
pub struct RewindIo<I> {
    io: I,
    read_buf: Option<Bytes>,
}

impl<I> RewindIo<I> {
    pub fn new(io: I) -> Self {
        Self { io, read_buf: None }
    }

    pub fn new_buffered(io: I, read_buf: Bytes) -> Self {
        Self {
            io,
            read_buf: Some(read_buf),
        }
    }

    pub fn get_ref(&self) -> &I {
        &self.io
    }

    pub fn get_mut(&mut self) -> &mut I {
        &mut self.io
    }

    pub fn into_parts(self) -> (I, Option<Bytes>) {
        (self.io, self.read_buf)
    }
}

impl<I: io::Read> io::Read for RewindIo<I> {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        if let Some(buf) = self.read_buf.take() {
            if !buf.is_empty() {
                let mut pre_reader = buf.into_buf().reader();
                let read_cnt = pre_reader.read(dst)?;

                let mut new_pre = pre_reader.into_inner().into_inner();
                new_pre.advance(read_cnt);

                if !new_pre.is_empty() {
                    self.read_buf = Some(new_pre);
                }

                return Ok(read_cnt);
            }
        }
        self.io.read(dst)
    }
}

impl<I: io::Write> io::Write for RewindIo<I> {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<I: AsyncRead> AsyncRead for RewindIo<I> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        if let Some(bs) = self.read_buf.take() {
            let pre_len = bs.len();
            if pre_len > 0 {
                let cnt = std::cmp::min(buf.remaining_mut(), pre_len);
                let pre_buf = bs.into_buf();
                let mut xfer = Buf::take(pre_buf, cnt);
                buf.put(&mut xfer);

                let mut new_pre = xfer.into_inner().into_inner();
                new_pre.advance(cnt);

                if !new_pre.is_empty() {
                    self.read_buf = Some(new_pre);
                }

                return Ok(Async::Ready(cnt));
            }
        }
        self.io.read_buf(buf)
    }
}

impl<I: AsyncWrite> AsyncWrite for RewindIo<I> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.io.write_buf(buf)
    }
}
