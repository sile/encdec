//! Encoders and decoders for combination.
//!
//! These are mainly created via the methods provided by `EncodeExt` or `DecodeExt` traits.
use std;
use std::cmp;
use std::iter;
use std::marker::PhantomData;

pub use chain::{DecoderChain, EncoderChain};

use {Decode, DecodeBuf, Encode, Eos, Error, ErrorKind, ExactBytesEncode, Result};
use bytes::BytesEncoder;
use io::encode_to_writer;

/// Combinator for converting decoded items to other values.
///
/// This is created by calling `DecodeExt::map` method.
#[derive(Debug)]
pub struct Map<D, T, F> {
    decoder: D,
    map: F,
    _item: PhantomData<T>,
}
impl<D: Decode, T, F> Map<D, T, F> {
    pub(crate) fn new(decoder: D, map: F) -> Self
    where
        F: Fn(D::Item) -> T,
    {
        Map {
            decoder,
            map,
            _item: PhantomData,
        }
    }
}
impl<D, T, F> Decode for Map<D, T, F>
where
    D: Decode,
    F: Fn(D::Item) -> T,
{
    type Item = T;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        track!(self.decoder.decode(buf)).map(|r| r.map(&self.map))
    }

    fn has_terminated(&self) -> bool {
        self.decoder.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.decoder.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.decoder.requiring_bytes_hint()
    }
}

/// Combinator for modifying encoding/decoding errors.
///
/// This is created by calling `{DecodeExt, EncodeExt}::map_err` method.
#[derive(Debug)]
pub struct MapErr<C, F, E> {
    codec: C,
    map_err: F,
    _error: PhantomData<E>,
}
impl<C, F, E> MapErr<C, F, E> {
    pub(crate) fn new(codec: C, map_err: F) -> Self
    where
        F: Fn(Error) -> E,
        Error: From<E>,
    {
        MapErr {
            codec,
            map_err,
            _error: PhantomData,
        }
    }
}
impl<D, F, E> Decode for MapErr<D, F, E>
where
    D: Decode,
    F: Fn(Error) -> E,
    Error: From<E>,
{
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        self.codec.decode(buf).map_err(|e| (self.map_err)(e).into())
    }

    fn has_terminated(&self) -> bool {
        self.codec.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.codec.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.codec.requiring_bytes_hint()
    }
}
impl<C, F, E> Encode for MapErr<C, F, E>
where
    C: Encode,
    F: Fn(Error) -> E,
    Error: From<E>,
{
    type Item = C::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        self.codec
            .encode(buf, eos)
            .map_err(|e| (self.map_err)(e).into())
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        self.codec
            .start_encoding(item)
            .map_err(|e| (self.map_err)(e).into())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.codec.requiring_bytes_hint()
    }

    fn is_idle(&self) -> bool {
        self.codec.is_idle()
    }
}
impl<C, F, E> ExactBytesEncode for MapErr<C, F, E>
where
    C: ExactBytesEncode,
    F: Fn(Error) -> E,
    Error: From<E>,
{
    fn requiring_bytes(&self) -> u64 {
        self.codec.requiring_bytes()
    }
}

/// Combinator for conditional decoding.
///
/// If the first item is successfully decoded,
/// it will start decoding the second item by using the decoder returned by `f` function.
///
/// This is created by calling `DecodeExt::and_then` method.
#[derive(Debug)]
pub struct AndThen<D0, D1, F> {
    decoder0: D0,
    decoder1: Option<D1>,
    and_then: F,
}
impl<D0: Decode, D1, F> AndThen<D0, D1, F> {
    pub(crate) fn new(decoder0: D0, and_then: F) -> Self
    where
        F: Fn(D0::Item) -> D1,
    {
        AndThen {
            decoder0,
            decoder1: None,
            and_then,
        }
    }
}
impl<D0, D1, F> Decode for AndThen<D0, D1, F>
where
    D0: Decode,
    D1: Decode,
    F: Fn(D0::Item) -> D1,
{
    type Item = D1::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        let mut item = None;
        loop {
            if let Some(ref mut d) = self.decoder1 {
                item = track!(d.decode(buf))?;
                break;
            } else if let Some(d) = track!(self.decoder0.decode(buf))?.map(&self.and_then) {
                self.decoder1 = Some(d);
            } else {
                break;
            }
        }
        if item.is_some() {
            self.decoder1 = None;
        }
        Ok(item)
    }

    fn has_terminated(&self) -> bool {
        if let Some(ref d) = self.decoder1 {
            d.has_terminated()
        } else {
            self.decoder0.has_terminated()
        }
    }

    fn is_idle(&self) -> bool {
        self.decoder1.is_none() && self.decoder0.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if let Some(ref d) = self.decoder1 {
            d.requiring_bytes_hint()
        } else {
            self.decoder0.requiring_bytes_hint()
        }
    }
}

/// Combinator for converting items into ones that
/// suited to the inner encoder by calling the given function.
///
/// This is created by calling `EncodeExt::map_from` method.
#[derive(Debug)]
pub struct MapFrom<E, T, F> {
    encoder: E,
    _item: PhantomData<T>,
    from: F,
}
impl<E, T, F> MapFrom<E, T, F> {
    pub(crate) fn new(encoder: E, from: F) -> Self {
        MapFrom {
            encoder,
            _item: PhantomData,
            from,
        }
    }
}
impl<E, T, F> Encode for MapFrom<E, T, F>
where
    E: Encode,
    F: Fn(T) -> E::Item,
{
    type Item = T;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        track!(self.encoder.encode(buf, eos))
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track!(self.encoder.start_encoding((self.from)(item)))
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.encoder.requiring_bytes_hint()
    }

    fn is_idle(&self) -> bool {
        self.encoder.is_idle()
    }
}
impl<E, T, F> ExactBytesEncode for MapFrom<E, T, F>
where
    E: ExactBytesEncode,
    F: Fn(T) -> E::Item,
{
    fn requiring_bytes(&self) -> u64 {
        self.encoder.requiring_bytes()
    }
}

/// Combinator that tries to convert items into ones that
/// suited to the inner encoder by calling the given function.
///
/// This is created by calling `EncodeExt::try_map_from` method.
#[derive(Debug)]
pub struct TryMapFrom<C, T, E, F> {
    encoder: C,
    try_from: F,
    _phantom: PhantomData<(T, E)>,
}
impl<C, T, E, F> TryMapFrom<C, T, E, F> {
    pub(crate) fn new(encoder: C, try_from: F) -> Self {
        TryMapFrom {
            encoder,
            try_from,
            _phantom: PhantomData,
        }
    }
}
impl<C, T, E, F> Encode for TryMapFrom<C, T, E, F>
where
    C: Encode,
    F: Fn(T) -> std::result::Result<C::Item, E>,
    Error: From<E>,
{
    type Item = T;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        track!(self.encoder.encode(buf, eos))
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        let item = track!((self.try_from)(item).map_err(Error::from))?;
        track!(self.encoder.start_encoding(item))
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.encoder.requiring_bytes_hint()
    }

    fn is_idle(&self) -> bool {
        self.encoder.is_idle()
    }
}
impl<C, T, E, F> ExactBytesEncode for TryMapFrom<C, T, E, F>
where
    C: ExactBytesEncode,
    F: Fn(T) -> std::result::Result<C::Item, E>,
    Error: From<E>,
{
    fn requiring_bytes(&self) -> u64 {
        self.encoder.requiring_bytes()
    }
}

/// Combinator for repeating encoding of `E::Item`.
///
/// This is created by calling `EncodeExt::repeat` method.
#[derive(Debug)]
pub struct Repeat<E, I> {
    encoder: E,
    items: Option<I>,
}
impl<E, I> Repeat<E, I> {
    pub(crate) fn new(encoder: E) -> Self {
        Repeat {
            encoder,
            items: None,
        }
    }
}
impl<E, I> Encode for Repeat<E, I>
where
    E: Encode,
    I: Iterator<Item = E::Item>,
{
    type Item = I;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        while self.encoder.is_idle() {
            if let Some(item) = self.items.as_mut().and_then(|iter| iter.next()) {
                track!(self.encoder.start_encoding(item))?;
            } else {
                self.items = None;
                break;
            }
        }
        track!(self.encoder.encode(buf, eos))
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track_assert!(self.is_idle(), ErrorKind::EncoderFull);
        self.items = Some(item);
        Ok(())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if self.is_idle() {
            Some(0)
        } else {
            None
        }
    }

    fn is_idle(&self) -> bool {
        self.items.is_none()
    }
}

/// Combinator for representing optional decoders.
///
/// This is created by calling `DecodeExt::omit` method.
#[derive(Debug)]
pub struct Omit<D>(Option<D>);
impl<D> Omit<D> {
    pub(crate) fn new(decoder: D, do_omit: bool) -> Self {
        if do_omit {
            Omit(None)
        } else {
            Omit(Some(decoder))
        }
    }
}
impl<D: Decode> Decode for Omit<D> {
    type Item = Option<D::Item>;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        if let Some(ref mut d) = self.0 {
            if let Some(item) = track!(d.decode(buf))? {
                Ok(Some(Some(item)))
            } else {
                Ok(None)
            }
        } else {
            Ok(Some(None))
        }
    }

    fn has_terminated(&self) -> bool {
        if let Some(ref d) = self.0 {
            d.has_terminated()
        } else {
            false
        }
    }

    fn is_idle(&self) -> bool {
        self.0.as_ref().map_or(true, |d| d.is_idle())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if let Some(ref d) = self.0 {
            d.requiring_bytes_hint()
        } else {
            Some(0)
        }
    }
}

/// Combinator for representing an optional encoder.
#[derive(Debug)]
pub struct Optional<E>(E);
impl<E> Optional<E> {
    pub(crate) fn new(encoder: E) -> Self {
        Optional(encoder)
    }
}
impl<E: Encode> Encode for Optional<E> {
    type Item = Option<E::Item>;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        track!(self.0.encode(buf, eos))
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        if let Some(item) = item {
            track!(self.0.start_encoding(item))?;
        }
        Ok(())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.0.requiring_bytes_hint()
    }

    fn is_idle(&self) -> bool {
        self.0.is_idle()
    }
}
impl<E: ExactBytesEncode> ExactBytesEncode for Optional<E> {
    fn requiring_bytes(&self) -> u64 {
        self.0.requiring_bytes()
    }
}

/// Combinator for collecting decoded items.
///
/// This is created by calling `DecodeExt::collect` method.
///
/// Note that this is a oneshot decoder (i.e., it decodes only one item).
#[derive(Debug)]
pub struct Collect<D, T> {
    decoder: D,
    items: Option<T>,
}
impl<D, T> Collect<D, T> {
    pub(crate) fn new(decoder: D) -> Self {
        Collect {
            decoder,
            items: None,
        }
    }
}
impl<D, T: Default> Decode for Collect<D, T>
where
    D: Decode,
    T: Extend<D::Item>,
{
    type Item = T;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        if self.items.is_none() {
            self.items = Some(T::default());
        }
        {
            let items = self.items.as_mut().expect("Never fails");
            while !(buf.is_empty() && buf.is_eos() || self.decoder.has_terminated()) {
                if let Some(item) = track!(self.decoder.decode(buf))? {
                    items.extend(iter::once(item));
                } else {
                    return Ok(None);
                }
            }
        }
        Ok(self.items.take())
    }

    fn has_terminated(&self) -> bool {
        self.decoder.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.items.is_none()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.decoder.requiring_bytes_hint()
    }
}

/// Combinator for consuming the specified number of bytes exactly.
///
/// This is created by calling `{DecodeExt, EncodeExt}::length` method.
#[derive(Debug)]
pub struct Length<C> {
    inner: C,
    expected_bytes: u64,
    remaining_bytes: u64,
}
impl<C> Length<C> {
    pub(crate) fn new(inner: C, expected_bytes: u64) -> Self {
        Length {
            inner,
            expected_bytes,
            remaining_bytes: expected_bytes,
        }
    }

    /// Returns the number of bytes expected to be consumed for decoding an item.
    pub fn expected_bytes(&self) -> u64 {
        self.expected_bytes
    }

    /// Sets the number of bytes expected to be consumed for decoding an item.
    ///
    /// # Errors
    ///
    /// If it is in the middle of decoding an item, it willl return an `ErrorKind::Other` error.
    pub fn set_expected_bytes(&mut self, bytes: u64) -> Result<()> {
        track_assert_eq!(
            self.remaining_bytes,
            self.expected_bytes,
            ErrorKind::Other,
            "An item is being decoded"
        );
        self.expected_bytes = bytes;
        self.remaining_bytes = bytes;
        Ok(())
    }

    /// Returns the number of remaining bytes required to decode the next item.
    pub fn remaining_bytes(&self) -> u64 {
        self.remaining_bytes
    }

    /// Returns a reference to the inner encoder or decoder.
    pub fn inner_ref(&self) -> &C {
        &self.inner
    }

    /// Returns a mutable reference to the inner encoder or decoder.
    pub fn inner_mut(&mut self) -> &mut C {
        &mut self.inner
    }
}
impl<D: Decode> Decode for Length<D> {
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        let old_buf_len = buf.len();
        let buf_len = cmp::min(buf.len() as u64, self.remaining_bytes) as usize;
        let expected_remaining_bytes = self.remaining_bytes - buf_len as u64;
        if let Some(remaining_bytes) = buf.remaining_bytes() {
            track_assert!(remaining_bytes >= expected_remaining_bytes, ErrorKind::UnexpectedEos;
                          remaining_bytes, expected_remaining_bytes);
        }
        let item = buf.with_limit_and_remaining_bytes(buf_len, expected_remaining_bytes, |buf| {
            track!(self.inner.decode(buf))
        })?;

        self.remaining_bytes -= (old_buf_len - buf.len()) as u64;
        if item.is_some() {
            track_assert_eq!(
                self.remaining_bytes,
                0,
                ErrorKind::Other,
                "Decoder consumes too few bytes"
            );
            self.remaining_bytes = self.expected_bytes
        }
        Ok(item)
    }

    fn has_terminated(&self) -> bool {
        if self.remaining_bytes == self.expected_bytes {
            self.inner.has_terminated()
        } else {
            false
        }
    }

    fn is_idle(&self) -> bool {
        self.remaining_bytes == self.expected_bytes && self.inner.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if self.has_terminated() {
            Some(0)
        } else {
            Some(self.remaining_bytes)
        }
    }
}
impl<E: Encode> Encode for Length<E> {
    type Item = E::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        if (buf.len() as u64) < self.remaining_bytes {
            track_assert!(!eos.is_eos(), ErrorKind::UnexpectedEos);
        }

        let (limit, eos) = if (buf.len() as u64) < self.remaining_bytes {
            (buf.len(), eos)
        } else {
            (self.remaining_bytes as usize, Eos::new(true))
        };
        let size = track!(self.inner.encode(&mut buf[..limit], eos))?;
        self.remaining_bytes -= size as u64;
        if self.inner.is_idle() {
            track_assert_eq!(
                self.remaining_bytes,
                0,
                ErrorKind::InvalidInput, // TODO: TooLittleConsumption
                "Too small item"
            );
        }
        Ok(size)
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track_assert_eq!(
            self.remaining_bytes,
            self.expected_bytes,
            ErrorKind::EncoderFull
        );
        self.remaining_bytes = self.expected_bytes;
        track!(self.inner.start_encoding(item))
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        Some(self.remaining_bytes)
    }

    fn is_idle(&self) -> bool {
        self.remaining_bytes == 0
    }
}
impl<E: Encode> ExactBytesEncode for Length<E> {
    fn requiring_bytes(&self) -> u64 {
        self.remaining_bytes
    }
}

/// Combinator for decoding the specified number of items.
///
/// This is created by calling `DecodeExt::take` method.
#[derive(Debug)]
pub struct Take<D> {
    decoder: D,
    limit: usize,
    decoded_items: usize,
}
impl<D> Take<D> {
    pub(crate) fn new(decoder: D, count: usize) -> Self {
        Take {
            decoder,
            limit: count,
            decoded_items: 0,
        }
    }
}
impl<D: Decode> Decode for Take<D> {
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        track_assert_ne!(self.decoded_items, self.limit, ErrorKind::DecoderTerminated);
        if let Some(item) = track!(self.decoder.decode(buf))? {
            self.decoded_items += 1;
            Ok(Some(item))
        } else {
            Ok(None)
        }
    }

    fn has_terminated(&self) -> bool {
        self.decoder.has_terminated() || self.decoded_items == self.limit
    }

    fn is_idle(&self) -> bool {
        self.decoded_items == 0 || self.decoded_items == self.limit
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if self.has_terminated() {
            Some(0)
        } else {
            self.decoder.requiring_bytes_hint()
        }
    }
}

/// Combinator which tries to convert decoded values by calling the specified function.
///
/// This is created by calling `DecodeExt::try_map` method.
#[derive(Debug)]
pub struct TryMap<D, F, T, E> {
    decoder: D,
    try_map: F,
    _phantom: PhantomData<(T, E)>,
}
impl<D, F, T, E> TryMap<D, F, T, E> {
    pub(crate) fn new(decoder: D, try_map: F) -> Self {
        TryMap {
            decoder,
            try_map,
            _phantom: PhantomData,
        }
    }
}
impl<D, F, T, E> Decode for TryMap<D, F, T, E>
where
    D: Decode,
    F: Fn(D::Item) -> std::result::Result<T, E>,
    Error: From<E>,
{
    type Item = T;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        if let Some(item) = track!(self.decoder.decode(buf))? {
            let item = track!((self.try_map)(item).map_err(Error::from))?;
            Ok(Some(item))
        } else {
            Ok(None)
        }
    }

    fn has_terminated(&self) -> bool {
        self.decoder.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.decoder.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.decoder.requiring_bytes_hint()
    }
}

/// Combinator for skipping the remaining bytes in an input byte sequence
/// after decoding an item by using `D`.
#[derive(Debug)]
pub struct SkipRemaining<D: Decode> {
    decoder: D,
    item: Option<D::Item>,
}
impl<D: Decode> SkipRemaining<D> {
    pub(crate) fn new(decoder: D) -> Self {
        SkipRemaining {
            decoder,
            item: None,
        }
    }
}
impl<D: Decode> Decode for SkipRemaining<D> {
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        track_assert!(
            buf.remaining_bytes().is_some(),
            ErrorKind::InvalidInput,
            "Cannot skip infinity byte stream"
        );

        if self.item.is_none() {
            self.item = track!(self.decoder.decode(buf))?;
        }
        if self.item.is_some() {
            buf.consume_all();
            if buf.is_eos() {
                return Ok(self.item.take());
            }
        }
        Ok(None)
    }

    fn has_terminated(&self) -> bool {
        if self.item.is_none() {
            self.decoder.has_terminated()
        } else {
            false
        }
    }

    fn is_idle(&self) -> bool {
        self.item.is_none() && self.decoder.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        if self.item.is_none() {
            self.decoder.requiring_bytes_hint()
        } else {
            None
        }
    }
}

/// Combinator that will fail if the number of consumed bytes exceeds the specified size.
///
/// This is created by calling `{DecodeExt, EncodeExt}::max_bytes` method.
#[derive(Debug)]
pub struct MaxBytes<C> {
    codec: C,
    consumed_bytes: u64,
    max_bytes: u64,
}
impl<C> MaxBytes<C> {
    pub(crate) fn new(codec: C, max_bytes: u64) -> Self {
        MaxBytes {
            codec,
            consumed_bytes: 0,
            max_bytes,
        }
    }

    fn max_remaining_bytes(&self) -> u64 {
        self.max_bytes - self.consumed_bytes
    }
}
impl<D: Decode> Decode for MaxBytes<D> {
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        let old_buf_len = buf.len();
        let actual_buf_len = cmp::min(buf.len() as u64, self.max_remaining_bytes()) as usize;
        let item = buf.with_limit(actual_buf_len, |buf| track!(self.codec.decode(buf)))?;
        self.consumed_bytes = (old_buf_len - buf.len()) as u64;
        if self.consumed_bytes == self.max_bytes {
            track_assert!(item.is_some(), ErrorKind::InvalidInput, "Max bytes limit exceeded";
                          self.max_bytes);
        }
        if item.is_some() {
            self.consumed_bytes = 0;
        }
        Ok(item)
    }

    fn has_terminated(&self) -> bool {
        self.codec.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.codec.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.codec.requiring_bytes_hint()
    }
}
impl<E: Encode> Encode for MaxBytes<E> {
    type Item = E::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        let limit = cmp::min(buf.len() as u64, self.max_remaining_bytes()) as usize;
        let eos = eos.back((buf.len() - limit) as u64);
        let size = track!(self.codec.encode(&mut buf[..limit], eos))?;
        self.consumed_bytes += size as u64;
        if self.consumed_bytes == self.max_bytes {
            track_assert!(self.is_idle(), ErrorKind::InvalidInput, "Max bytes limit exceeded";
                          self.max_bytes);
        }
        if self.is_idle() {
            self.consumed_bytes = 0;
        }
        Ok(size)
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track!(self.codec.start_encoding(item))
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.codec.requiring_bytes_hint()
    }

    fn is_idle(&self) -> bool {
        self.codec.is_idle()
    }
}
impl<E: ExactBytesEncode> ExactBytesEncode for MaxBytes<E> {
    fn requiring_bytes(&self) -> u64 {
        self.codec.requiring_bytes()
    }
}

/// Combinator for declaring an assertion about decoded items.
///
/// This created by calling `DecodeExt::assert` method.
#[derive(Debug)]
pub struct Assert<D, F> {
    decoder: D,
    assert: F,
}
impl<D, F> Assert<D, F> {
    pub(crate) fn new(decoder: D, assert: F) -> Self {
        Assert { decoder, assert }
    }
}
impl<D: Decode, F> Decode for Assert<D, F>
where
    F: for<'a> Fn(&'a D::Item) -> bool,
{
    type Item = D::Item;

    fn decode(&mut self, buf: &mut DecodeBuf) -> Result<Option<Self::Item>> {
        if let Some(item) = track!(self.decoder.decode(buf))? {
            track_assert!((self.assert)(&item), ErrorKind::InvalidInput);
            Ok(Some(item))
        } else {
            Ok(None)
        }
    }

    fn has_terminated(&self) -> bool {
        self.decoder.has_terminated()
    }

    fn is_idle(&self) -> bool {
        self.decoder.is_idle()
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        self.decoder.requiring_bytes_hint()
    }
}

/// Combinator that keeps writing padding byte until it reaches EOS
/// after encoding of `E`'s item has been completed.
///
/// This is created by calling `EncodeExt::padding` method.
#[derive(Debug)]
pub struct Padding<E> {
    encoder: E,
    padding_byte: u8,
    eos_reached: bool,
}
impl<E> Padding<E> {
    pub(crate) fn new(encoder: E, padding_byte: u8) -> Self {
        Padding {
            encoder,
            padding_byte,
            eos_reached: true,
        }
    }
}
impl<E: Encode> Encode for Padding<E> {
    type Item = E::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        if !self.encoder.is_idle() {
            self.encoder.encode(buf, eos)
        } else {
            for b in buf.iter_mut() {
                *b = self.padding_byte;
            }
            self.eos_reached = eos.is_eos();
            Ok(buf.len())
        }
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track_assert!(self.is_idle(), ErrorKind::EncoderFull);
        self.eos_reached = false;
        track!(self.encoder.start_encoding(item))
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        None
    }

    fn is_idle(&self) -> bool {
        self.eos_reached
    }
}

/// Combinator for adding prefix items.
///
/// This is created by calling `EncodeExt::with_prefix` method.
#[derive(Debug)]
pub struct WithPrefix<E0, E1, F> {
    body_encoder: E0,
    prefix_encoder: E1,
    with_prefix: F,
}
impl<E0, E1, F> WithPrefix<E0, E1, F> {
    pub(crate) fn new(body_encoder: E0, prefix_encoder: E1, with_prefix: F) -> Self {
        WithPrefix {
            body_encoder,
            prefix_encoder,
            with_prefix,
        }
    }
}
impl<E0, E1, F> Encode for WithPrefix<E0, E1, F>
where
    E0: Encode,
    E1: Encode,
    F: Fn(&E0) -> E1::Item,
{
    type Item = E0::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        if !self.prefix_encoder.is_idle() {
            track!(self.prefix_encoder.encode(buf, eos))
        } else {
            track!(self.body_encoder.encode(buf, eos))
        }
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        track_assert!(self.is_idle(), ErrorKind::EncoderFull);
        track!(self.body_encoder.start_encoding(item))?;
        let prefix_item = (self.with_prefix)(&self.body_encoder);
        track!(self.prefix_encoder.start_encoding(prefix_item))?;
        Ok(())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        let a = self.prefix_encoder.requiring_bytes_hint();
        let b = self.body_encoder.requiring_bytes_hint();
        match (a, b) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    fn is_idle(&self) -> bool {
        self.prefix_encoder.is_idle() && self.body_encoder.is_idle()
    }
}
impl<E0, E1, F> ExactBytesEncode for WithPrefix<E0, E1, F>
where
    E0: ExactBytesEncode,
    E1: ExactBytesEncode,
    F: Fn(&E0) -> E1::Item,
{
    fn requiring_bytes(&self) -> u64 {
        self.prefix_encoder.requiring_bytes() + self.body_encoder.requiring_bytes()
    }
}

/// Combinator for pre-encoding items when `start_encoding` method is called.
///
/// This is created by calling `EncodeExt::pre_encode` method.
#[derive(Debug)]
pub struct PreEncode<E> {
    encoder: E,
    pre_encoded: BytesEncoder<Vec<u8>>,
}
impl<E> PreEncode<E> {
    pub(crate) fn new(encoder: E) -> Self {
        PreEncode {
            encoder,
            pre_encoded: BytesEncoder::new(),
        }
    }
}
impl<E: Encode> Encode for PreEncode<E> {
    type Item = E::Item;

    fn encode(&mut self, buf: &mut [u8], eos: Eos) -> Result<usize> {
        track!(self.pre_encoded.encode(buf, eos))
    }

    fn start_encoding(&mut self, item: Self::Item) -> Result<()> {
        let mut buf = Vec::new();
        track!(encode_to_writer(&mut self.encoder, item, &mut buf))?;
        track!(self.pre_encoded.start_encoding(buf))?;
        Ok(())
    }

    fn requiring_bytes_hint(&self) -> Option<u64> {
        Some(self.requiring_bytes())
    }

    fn is_idle(&self) -> bool {
        self.pre_encoded.is_idle()
    }
}
impl<E: Encode> ExactBytesEncode for PreEncode<E> {
    fn requiring_bytes(&self) -> u64 {
        self.pre_encoded.requiring_bytes()
    }
}

#[cfg(test)]
mod test {
    use {Decode, DecodeBuf, DecodeExt, Encode, EncodeExt, ErrorKind};
    use bytes::{Utf8Decoder, Utf8Encoder};
    use fixnum::{U8Decoder, U8Encoder};

    #[test]
    fn collect_works() {
        let mut decoder = U8Decoder::new().collect::<Vec<_>>();
        let mut input = DecodeBuf::with_remaining_bytes(b"foo", 0);

        let item = track_try_unwrap!(decoder.decode(&mut input));
        assert_eq!(item, Some(vec![b'f', b'o', b'o']));
    }

    #[test]
    fn take_works() {
        let mut decoder = U8Decoder::new().take(2).collect::<Vec<_>>();
        let mut input = DecodeBuf::new(b"foo");

        let item = track_try_unwrap!(decoder.decode(&mut input));
        assert_eq!(item, Some(vec![b'f', b'o']));
    }

    #[test]
    fn decoder_length_works() {
        let mut decoder = Utf8Decoder::new().length(3);
        let mut input = DecodeBuf::with_remaining_bytes(b"foobarba", 0);

        let item = track_try_unwrap!(decoder.decode(&mut input));
        assert_eq!(item, Some("foo".to_owned()));

        let item = track_try_unwrap!(decoder.decode(&mut input));
        assert_eq!(item, Some("bar".to_owned()));

        let error = decoder.decode(&mut input).err().unwrap();
        assert_eq!(*error.kind(), ErrorKind::UnexpectedEos);
    }

    #[test]
    fn encoder_length_works() {
        let mut output = [0; 4];
        let mut encoder = Utf8Encoder::new().length(3);
        encoder.start_encoding("hey").unwrap(); // OK
        track_try_unwrap!(encoder.encode_all(&mut output));
        assert_eq!(output.as_ref(), b"hey\x00");

        let mut output = [0; 4];
        let mut encoder = Utf8Encoder::new().length(3);
        encoder.start_encoding("hello").unwrap(); // Error (too long)
        let error = encoder.encode_all(&mut output).err().expect("too long");
        assert_eq!(*error.kind(), ErrorKind::UnexpectedEos);

        let mut output = [0; 4];
        let mut encoder = Utf8Encoder::new().length(3);
        encoder.start_encoding("hi").unwrap(); // Error (too short)
        let error = encoder.encode_all(&mut output).err().expect("too short");
        assert_eq!(*error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn padding_works() {
        let mut output = [0; 4];
        let mut encoder = U8Encoder::new().padding(9).length(3);
        encoder.start_encoding(3).unwrap();
        track_try_unwrap!(encoder.encode_all(&mut output[..]));
        assert_eq!(output.as_ref(), [3, 9, 9, 0]);
    }

    #[test]
    fn repeat_works() {
        let mut output = [0; 4];
        let mut encoder = U8Encoder::new().repeat();
        encoder.start_encoding(0..4).unwrap();
        track_try_unwrap!(encoder.encode_all(&mut output));
        assert_eq!(output.as_ref(), [0, 1, 2, 3]);
    }

    #[test]
    fn encoder_max_bytes_works() {
        let mut output = [0; 4];
        let mut encoder = Utf8Encoder::new().max_bytes(3);

        encoder.start_encoding("foo").unwrap(); // OK
        encoder.encode_all(&mut output).unwrap();
        assert_eq!(output.as_ref(), b"foo\x00");

        encoder.start_encoding("hello").unwrap(); // Error
        let error = encoder.encode_all(&mut output).err().unwrap();
        assert_eq!(*error.kind(), ErrorKind::InvalidInput);
    }
}
