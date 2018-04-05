use std::cmp;

/// Number of bytes of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum ByteCount {
    Finite(u64),
    Infinite,
    Unknown,
}
impl ByteCount {
    /// Returns `true` if this is `ByteCount::Finite(_)`, otherwise `false`.
    pub fn is_finite(&self) -> bool {
        if let ByteCount::Finite(_) = *self {
            true
        } else {
            false
        }
    }

    /// Returns `true` if this is `ByteCount::Infinite`, otherwise `false`.
    pub fn is_infinite(&self) -> bool {
        *self == ByteCount::Infinite
    }

    /// Returns `true` if this is `ByteCount::Unknown`, otherwise `false`.
    pub fn is_unknow(&self) -> bool {
        *self == ByteCount::Unknown
    }

    /// Tries to convert this `ByteCount` to an `u64` value.
    ///
    /// If it is not a `ByteCount::Finite(_)`,`None` will be returned.
    pub fn to_u64(&self) -> Option<u64> {
        if let ByteCount::Finite(n) = *self {
            Some(n)
        } else {
            None
        }
    }
}
impl PartialOrd for ByteCount {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        match (*self, *other) {
            (ByteCount::Finite(l), ByteCount::Finite(r)) => Some(l.cmp(&r)),
            (ByteCount::Unknown, _) | (_, ByteCount::Unknown) => None,
            (ByteCount::Infinite, ByteCount::Infinite) => Some(cmp::Ordering::Equal),
            (ByteCount::Infinite, _) => Some(cmp::Ordering::Greater),
            (_, ByteCount::Infinite) => Some(cmp::Ordering::Less),
        }
    }
}
