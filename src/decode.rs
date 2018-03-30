use std;
use std::cmp;
use std::io::{self, Read};
use std::ops::Deref;

use {Error, ErrorKind, Result};
use combinator::{AndThen, Collect, DecoderChain, IgnoreRest, Map, MapErr, Take, Validate};

/// This trait allows for decoding items from a byte sequence incrementally.
pub trait Decode {
    /// The type of items to be decoded.
    type Item;

    /// Consumes the given buffer (a part of a byte sequence), and decodes an item from it.
    ///
    /// If an item is successfully decoded, the decoder will return `Ok(Some(..))`.
    ///
    /// If the buffer does not contain enough bytes to decode the next item,
    /// the decoder will return `Ok(None)`.
    /// In this case, the decoder **must** consume all the bytes in the buffer.
    ///
    /// Finally, if there are no items to be decoded anymore, the decoder will return `Ok(None)`.
    /// In this case, the one or more bytes in the buffer may be consumed
    /// for detecting the termination.
    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>>;

    /// Returns the lower bound of the number of bytes needed to decode the next item.
    ///
    /// If the decoder does not know the value, it will return `None`
    /// (e.g., null-terminated strings have no pre-estimable length).
    ///
    /// If the decoder returns `Some(0)`, it means one of the followings:
    /// - (a) There is an already decoded item
    ///   - The next invocation of `decode()` will return it without consuming any bytes
    /// - (b) There are no decodable items
    ///   - All decodable items have been decoded, and the decoder has no further works
    fn requiring_bytes_hint(&self) -> Option<u64>;
}
impl<D: ?Sized + Decode> Decode for Box<D> {
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        (**self).decode(buf)
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        (**self).requiring_bytes_hint()
    }
}

// TODO: Immediate or Value

// TODO: remove or rename
impl<D: Decode> Decode for Option<D> {
    type Item = Option<D::Item>;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        if let Some(ref mut d) = *self {
            Ok(track!(d.decode(buf))?.map(Some))
        } else {
            Ok(None)
        }
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.as_ref().map_or(Some(0), |d| d.requiring_bytes_hint())
    }
}

pub trait DecodeExt: Decode + Sized {
    fn map<T, F>(self, f: F) -> Map<Self, T, F>
    where
        F: Fn(Self::Item) -> T,
    {
        Map::new(self, f)
    }

    fn map_err<F>(self, f: F) -> MapErr<Self, F>
    where
        F: Fn(Error) -> Error,
    {
        MapErr::new(self, f)
    }

    fn and_then<D, F>(self, f: F) -> AndThen<Self, D, F>
    where
        F: Fn(Self::Item) -> D,
        D: Decode,
    {
        AndThen::new(self, f)
    }

    fn chain<D: Decode>(self, other: D) -> DecoderChain<Self, D> {
        DecoderChain::new(self, other)
    }

    fn collect<T>(self) -> Collect<Self, T>
    where
        T: Extend<Self::Item> + Default,
    {
        Collect::new(self)
    }

    fn take(self, size: u64) -> Take<Self> {
        Take::new(self, size)
    }

    fn present(self, b: bool) -> Option<Self> {
        if b {
            Some(self)
        } else {
            None
        }
    }

    fn ignore_rest(self) -> IgnoreRest<Self> {
        IgnoreRest::new(self)
    }

    fn validate<F, E>(self, f: F) -> Validate<Self, F, E>
    where
        F: for<'a> Fn(&'a Self::Item) -> std::result::Result<(), E>,
        Error: From<E>,
    {
        Validate::new(self, f)
    }

    // TODO: min, max
    // TODO: max_bytes
}
impl<T: Decode> DecodeExt for T {}

#[derive(Debug)]
pub struct DecodeBuf<'a> {
    buf: &'a [u8],
    offset: usize,
    remaining_bytes: Option<u64>,
}
impl DecodeBuf<'static> {
    pub fn eos() -> Self {
        DecodeBuf {
            buf: &[],
            offset: 0,
            remaining_bytes: Some(0),
        }
    }
}
impl<'a> DecodeBuf<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        DecodeBuf {
            buf,
            offset: 0,
            remaining_bytes: None,
        }
    }

    pub fn with_remaining_bytes(buf: &'a [u8], remaining_bytes: u64) -> Self {
        DecodeBuf {
            buf,
            offset: 0,
            remaining_bytes: Some(remaining_bytes),
        }
    }

    // buf.len()に加えて、次のitemをデコードするのに必要なバイト数.
    // 不明な場合には`None`
    pub fn remaining_bytes(&self) -> Option<u64> {
        self.remaining_bytes
    }

    pub fn is_eos(&self) -> bool {
        self.remaining_bytes().map_or(false, |n| n == 0)
    }

    pub fn consume(&mut self, size: usize) -> Result<()> {
        track_assert!(self.offset + size <= self.buf.len(), ErrorKind::InvalidInput;
                      self.offset, size, self.buf.len());
        self.offset += size;
        Ok(())
    }
}
impl<'a> AsRef<[u8]> for DecodeBuf<'a> {
    fn as_ref(&self) -> &[u8] {
        &self.buf[self.offset..]
    }
}
impl<'a> Deref for DecodeBuf<'a> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
impl<'a> Read for DecodeBuf<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size = cmp::min(self.len(), buf.len());
        (&mut buf[..size]).copy_from_slice(&self[..size]);
        self.offset += size;
        Ok(size)
    }
}
