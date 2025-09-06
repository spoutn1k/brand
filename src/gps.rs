use tiff::{encoder::Rational, tags::Tag};

pub trait GpsRef {
    fn format(&self) -> Vec<Rational>;
}

impl GpsRef for f64 {
    /// Converts a GPS coordinate to the format expected by the EXIF standard
    fn format(&self) -> Vec<Rational> {
        let deg = Rational {
            n: *self as u32,
            d: 1,
        };
        let min = Rational {
            n: (self.fract() * 60.0) as u32,
            d: 1,
        };

        let sec_raw = (self * 60.0).fract() * 60.0;

        let sec = match num_rational::Ratio::<u32>::approximate_float_unsigned(sec_raw) {
            Some(sec) => Rational {
                n: *sec.numer(),
                d: *sec.denom(),
            },
            None => Rational {
                n: sec_raw as u32,
                d: 1,
            },
        };

        vec![deg, min, sec]
    }
}

/// Tag space of GPS ifds
pub enum GpsTag {
    //GPSVersionID,
    GPSLatitudeRef,
    GPSLatitude,
    GPSLongitudeRef,
    GPSLongitude,
}

impl From<GpsTag> for Tag {
    fn from(tag: GpsTag) -> Self {
        Tag::Unknown(u16::from(tag))
    }
}

impl From<GpsTag> for u16 {
    fn from(tag: GpsTag) -> Self {
        match tag {
            //GpsTag::GPSVersionID => 0x0000,
            GpsTag::GPSLatitudeRef => 0x0001,
            GpsTag::GPSLatitude => 0x0002,
            GpsTag::GPSLongitudeRef => 0x0003,
            GpsTag::GPSLongitude => 0x0004,
        }
    }
}
