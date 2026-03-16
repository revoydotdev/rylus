/// Provides pixel data from a capture source in various formats.
pub enum PixelProvider<'a> {
    // 8 bits per color
    RGB(usize, usize, &'a [u8]),
    RGB0(usize, usize, &'a [u8]),
    BGR0(usize, usize, &'a [u8]),
    // width, height, stride
    BGR0S(usize, usize, usize, &'a [u8]),
}

impl<'a> PixelProvider<'a> {
    /// Returns `(width, height)` of the pixel data.
    pub fn size(&self) -> (usize, usize) {
        match self {
            PixelProvider::RGB(w, h, _) => (*w, *h),
            PixelProvider::RGB0(w, h, _) => (*w, *h),
            PixelProvider::BGR0(w, h, _) => (*w, *h),
            PixelProvider::BGR0S(w, h, _, _) => (*w, *h),
        }
    }

    /// Create an owned copy of the pixel data that can be sent across threads.
    pub fn to_owned(&self) -> OwnedPixelData {
        match self {
            PixelProvider::RGB(w, h, d) => OwnedPixelData::RGB(*w, *h, d.to_vec()),
            PixelProvider::RGB0(w, h, d) => OwnedPixelData::RGB0(*w, *h, d.to_vec()),
            PixelProvider::BGR0(w, h, d) => OwnedPixelData::BGR0(*w, *h, d.to_vec()),
            PixelProvider::BGR0S(w, h, s, d) => OwnedPixelData::BGR0S(*w, *h, *s, d.to_vec()),
        }
    }
}

/// Owned version of [`PixelProvider`] that can be sent across threads.
pub enum OwnedPixelData {
    RGB(usize, usize, Vec<u8>),
    RGB0(usize, usize, Vec<u8>),
    BGR0(usize, usize, Vec<u8>),
    BGR0S(usize, usize, usize, Vec<u8>),
}

impl OwnedPixelData {
    pub fn size(&self) -> (usize, usize) {
        match self {
            OwnedPixelData::RGB(w, h, _) => (*w, *h),
            OwnedPixelData::RGB0(w, h, _) => (*w, *h),
            OwnedPixelData::BGR0(w, h, _) => (*w, *h),
            OwnedPixelData::BGR0S(w, h, _, _) => (*w, *h),
        }
    }

    /// Borrow as a [`PixelProvider`].
    pub fn as_provider(&self) -> PixelProvider<'_> {
        match self {
            OwnedPixelData::RGB(w, h, d) => PixelProvider::RGB(*w, *h, d),
            OwnedPixelData::RGB0(w, h, d) => PixelProvider::RGB0(*w, *h, d),
            OwnedPixelData::BGR0(w, h, d) => PixelProvider::BGR0(*w, *h, d),
            OwnedPixelData::BGR0S(w, h, s, d) => PixelProvider::BGR0S(*w, *h, *s, d),
        }
    }
}
