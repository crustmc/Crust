use std::io::{Cursor, Read, Write};

use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};

use crate::util::{IOError, IOErrorKind, IOResult, VarInt};

pub fn compress(data: &[u8], threshold: i32, dest: &mut Vec<u8>) -> IOResult<bool> {
    if data.len() < threshold as usize {
        VarInt(0).encode(dest, 5)?; // no compression / uncompressed length

        dest.extend_from_slice(&data);
        Ok(false)
    } else {
        VarInt(data.len() as i32).encode(dest, 5)?; // uncompressed length

        let mut encoder = ZlibEncoder::new(dest, Compression::default());
        encoder.write_all(data)?;
        encoder.finish()?;
        Ok(true)
    }
}

pub fn decompress(data: &[u8], dest: &mut Vec<u8>) -> IOResult<()> {
    let mut reader = Cursor::new(data);
    let uncompressed_length = VarInt::decode(&mut reader, 5)?.get() as usize;
    if uncompressed_length == 0 {
        dest.extend_from_slice(&data[reader.position() as usize..]);
    } else {
        let mut decoder = SizeLimitedReader::new(ZlibDecoder::new(reader), 8 * 1024 * 1024);
        decoder.read_to_end(dest)?;
    }
    Ok(())
}

pub struct SizeLimitedReader<R: Read> {
    inner: R,
    remaining: usize,
}

impl<R: Read> SizeLimitedReader<R> {
    pub fn new(inner: R, limit: usize) -> Self {
        Self { inner, remaining: limit }
    }
}

impl<R: Read> Read for SizeLimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IOResult<usize> {
        if self.remaining == 0 {
            return Err(IOError::new(IOErrorKind::InvalidData, "exeeded length limit")); // Stop reading if limit is reached
        }

        let max_read = std::cmp::min(self.remaining, buf.len());
        let bytes_read = self.inner.read(&mut buf[..max_read])?;
        self.remaining -= bytes_read;
        Ok(bytes_read)
    }
}

pub struct RefSizeLimitedReader<'a, R: Read + ?Sized> {
    inner: &'a mut R,
    remaining: usize,
}

impl<'a, R: Read + ?Sized> RefSizeLimitedReader<'a, R> {
    pub fn new(inner: &'a mut R, limit: usize) -> Self {
        Self { inner, remaining: limit }
    }
}

impl<'a, R: Read + ?Sized> Read for RefSizeLimitedReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> IOResult<usize> {
        if self.remaining == 0 {
            return Err(IOError::new(IOErrorKind::InvalidData, "exeeded length limit")); // Stop reading if limit is reached
        }

        let max_read = std::cmp::min(self.remaining, buf.len());
        let bytes_read = self.inner.read(&mut buf[..max_read])?;
        self.remaining -= bytes_read;
        Ok(bytes_read)
    }
}
