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
    //GPSAltitudeRef,
    //GPSAltitude,
    //GPSTimeStamp,
    //GPSSatellites,
    //GPSStatus,
    //GPSMeasureMode,
    //GPSDOP,
    //GPSSpeedRef,
    //GPSSpeed,
    //GPSTrackRef,
    //GPSTrack,
    //GPSImgDirectionRef,
    //GPSImgDirection,
    //GPSMapDatum,
    //GPSDestLatitudeRef,
    //GPSDestLatitude,
    //GPSDestLongitudeRef,
    //GPSDestLongitude,
    //GPSDestBearingRef,
    //GPSDestBearing,
    //GPSDestDistanceRef,
    //GPSDestDistance,
    //GPSProcessingMethod,
    //GPSAreaInformation,
    //GPSDateStamp,
    //GPSDifferential,
    //GPSHPositioningError,
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
            //GpsTag::GPSAltitudeRef => 0x0005,
            //GpsTag::GPSAltitude => 0x0006,
            //GpsTag::GPSTimeStamp => 0x0007,
            //GpsTag::GPSSatellites => 0x0008,
            //GpsTag::GPSStatus => 0x0009,
            //GpsTag::GPSMeasureMode => 0x000a,
            //GpsTag::GPSDOP => 0x000b,
            //GpsTag::GPSSpeedRef => 0x000c,
            //GpsTag::GPSSpeed => 0x000d,
            //GpsTag::GPSTrackRef => 0x000e,
            //GpsTag::GPSTrack => 0x000f,
            //GpsTag::GPSImgDirectionRef => 0x0010,
            //GpsTag::GPSImgDirection => 0x0011,
            //GpsTag::GPSMapDatum => 0x0012,
            //GpsTag::GPSDestLatitudeRef => 0x0013,
            //GpsTag::GPSDestLatitude => 0x0014,
            //GpsTag::GPSDestLongitudeRef => 0x0015,
            //GpsTag::GPSDestLongitude => 0x0016,
            //GpsTag::GPSDestBearingRef => 0x0017,
            //GpsTag::GPSDestBearing => 0x0018,
            //GpsTag::GPSDestDistanceRef => 0x0019,
            //GpsTag::GPSDestDistance => 0x001a,
            //GpsTag::GPSProcessingMethod => 0x001b,
            //GpsTag::GPSAreaInformation => 0x001c,
            //GpsTag::GPSDateStamp => 0x001d,
            //GpsTag::GPSDifferential => 0x001e,
            //GpsTag::GPSHPositioningError => 0x001f,
        }
    }
}
