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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_provider_rgb_size() {
        let data = vec![0u8; 1920 * 1080 * 3];
        let pp = PixelProvider::RGB(1920, 1080, &data);
        assert_eq!(pp.size(), (1920, 1080));
    }

    #[test]
    fn pixel_provider_rgb0_size() {
        let data = vec![0u8; 800 * 600 * 4];
        let pp = PixelProvider::RGB0(800, 600, &data);
        assert_eq!(pp.size(), (800, 600));
    }

    #[test]
    fn pixel_provider_bgr0_size() {
        let data = vec![0u8; 640 * 480 * 4];
        let pp = PixelProvider::BGR0(640, 480, &data);
        assert_eq!(pp.size(), (640, 480));
    }

    #[test]
    fn pixel_provider_bgr0s_size() {
        let data = vec![0u8; 2560 * 1440 * 4];
        let pp = PixelProvider::BGR0S(2560, 1440, 2560 * 4, &data);
        assert_eq!(pp.size(), (2560, 1440));
    }

    #[test]
    fn pixel_provider_to_owned_preserves_data() {
        let data = vec![1u8, 2, 3, 4, 5, 6];
        let pp = PixelProvider::RGB(2, 1, &data);
        let owned = pp.to_owned();
        assert_eq!(owned.size(), (2, 1));
        match &owned {
            OwnedPixelData::RGB(w, h, d) => {
                assert_eq!(*w, 2);
                assert_eq!(*h, 1);
                assert_eq!(d, &data);
            }
            _ => panic!("Expected RGB variant"),
        }
    }

    #[test]
    fn pixel_provider_to_owned_bgr0s_preserves_stride() {
        let stride = 2048;
        let data = vec![0u8; stride * 100];
        let pp = PixelProvider::BGR0S(500, 100, stride, &data);
        let owned = pp.to_owned();
        match &owned {
            OwnedPixelData::BGR0S(w, h, s, d) => {
                assert_eq!(*w, 500);
                assert_eq!(*h, 100);
                assert_eq!(*s, stride);
                assert_eq!(d.len(), data.len());
            }
            _ => panic!("Expected BGR0S variant"),
        }
    }

    #[test]
    fn owned_pixel_data_as_provider_roundtrip() {
        let data = vec![10u8, 20, 30, 40, 50, 60, 70, 80];
        let pp = PixelProvider::RGB0(2, 1, &data);
        let owned = pp.to_owned();
        let provider = owned.as_provider();
        assert_eq!(provider.size(), (2, 1));
        match provider {
            PixelProvider::RGB0(_, _, d) => assert_eq!(d, &data[..]),
            _ => panic!("Expected RGB0"),
        }
    }

    #[test]
    fn owned_pixel_data_size_all_variants() {
        assert_eq!(OwnedPixelData::RGB(10, 20, vec![]).size(), (10, 20));
        assert_eq!(OwnedPixelData::RGB0(30, 40, vec![]).size(), (30, 40));
        assert_eq!(OwnedPixelData::BGR0(50, 60, vec![]).size(), (50, 60));
        assert_eq!(OwnedPixelData::BGR0S(70, 80, 280, vec![]).size(), (70, 80));
    }

    #[test]
    fn pixel_provider_to_owned_is_independent_copy() {
        let mut data = vec![1u8, 2, 3];
        let owned = {
            let pp = PixelProvider::RGB(1, 1, &data);
            pp.to_owned()
        };
        // Mutate original data
        data[0] = 255;
        // Owned copy should be unaffected
        match &owned {
            OwnedPixelData::RGB(_, _, d) => assert_eq!(d[0], 1),
            _ => panic!("Expected RGB"),
        }
    }
}
