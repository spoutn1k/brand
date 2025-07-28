use brand::image_management::{SupportedImage, format};
use chrono::{DateTime, NaiveDateTime};
use clap::Parser;
use image::{ImageEncoder, ImageReader, codecs::jpeg::JpegEncoder};
use regex::Regex;
use simple_logger::SimpleLogger;
use std::{collections::HashMap, error::Error, fs::File, io, path::PathBuf};
use tiff::{
    encoder::{Compression, Predictor, Rational, TiffEncoder, colortype},
    tags::Tag,
};
use winnow::{
    ModalResult, Parser as _,
    ascii::{alphanumeric1, float, tab},
    combinator::{alt, empty, opt, separated_pair, seq},
    error::{StrContext, StrContextValue},
    token::take_till,
};

static TIMESTAMP_FORMAT: &str = "%Y %m %d %H %M %S";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the exposure folder containing the index.tse file
    #[arg(required = true)]
    #[clap(default_value = ".")]
    dir: PathBuf,

    /// Turn debugging information on
    #[arg(short, long)]
    #[clap(default_value = "false")]
    debug: bool,
}

#[derive(Clone, Default, Debug)]
struct ExposureData {
    author: Option<String>,
    make: Option<String>,
    model: Option<String>,
    sspeed: Option<String>,
    aperture: Option<String>,
    iso: Option<String>,
    lens: Option<String>,
    description: Option<String>,
    comment: Option<String>,
    date: Option<NaiveDateTime>,
    gps: Option<(f64, f64)>,
}

trait GpsRef {
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
enum GpsTag {
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

impl ExposureData {
    fn complete(self, other: &Self) -> Self {
        ExposureData {
            author: self.author.or(other.author.clone()),
            make: self.make.or(other.make.clone()),
            model: self.model.or(other.model.clone()),
            sspeed: self.sspeed.or(other.sspeed.clone()),
            aperture: self.aperture.or(other.aperture.clone()),
            iso: self.iso.or(other.iso.clone()),
            lens: self.lens.or(other.lens.clone()),
            description: self.description.or(other.description.clone()),
            comment: self.comment.or(other.comment.clone()),
            date: self.date.or(other.date),
            gps: self.gps.or(other.gps),
        }
    }

    fn encode_exif<W: std::io::Write + std::io::Seek>(
        &self,
        encoder: &mut TiffEncoder<W>,
    ) -> Result<(), tiff::TiffError> {
        let gps_off = &self
            .gps
            .map(|coords| {
                let mut gps = encoder.extra_directory()?;

                let (lat, lon): (f64, f64);
                if coords.0 < 0.0 {
                    gps.write_tag(GpsTag::GPSLatitudeRef.into(), "S")?;
                    lat = -coords.0;
                } else {
                    gps.write_tag(GpsTag::GPSLatitudeRef.into(), "N")?;
                    lat = coords.0;
                }

                if coords.1 < 0.0 {
                    gps.write_tag(GpsTag::GPSLongitudeRef.into(), "W")?;
                    lon = -coords.1;
                } else {
                    gps.write_tag(GpsTag::GPSLongitudeRef.into(), "E")?;
                    lon = coords.1;
                }

                gps.write_tag(GpsTag::GPSLatitude.into(), lat.format().as_slice())?;
                gps.write_tag(GpsTag::GPSLongitude.into(), lon.format().as_slice())?;

                gps.finish_with_offsets()
            })
            .and_then(|r| r.ok());

        let mut dir = encoder.image_directory()?;
        if let Some(off) = gps_off {
            dir.write_tag(Tag::GpsDirectory, off.offset)?;
        }

        if let Some(value) = &self.author {
            dir.write_tag(Tag::Artist, value.as_str())?;
        }

        if let Some(make) = &self.make {
            dir.write_tag(Tag::Make, make.as_str())?;
        }

        if let Some(model) = &self.model {
            dir.write_tag(Tag::Model, model.as_str())?;
        }

        match (&self.description, &self.comment) {
            (Some(description), Some(comment)) => {
                dir.write_tag(
                    Tag::ImageDescription,
                    format!("{description} - {comment}").as_str(),
                )?;
            }
            (Some(description), None) => {
                dir.write_tag(Tag::ImageDescription, description.as_str())?;
            }
            (None, Some(comment)) => {
                dir.write_tag(Tag::ImageDescription, comment.as_str())?;
            }
            _ => (),
        }

        if let Some(iso) = &self.iso {
            let iso = iso.parse::<u16>().unwrap();
            dir.write_tag(Tag::Unknown(0x8827), iso)?;
        }

        if let Some(shutter_speed) = &self.sspeed {
            dir.write_tag(Tag::Unknown(0x9201), shutter_speed.parse::<u16>().unwrap())?;
        }

        dir.finish()
    }
}

pub fn expected(reason: &'static str) -> StrContext {
    StrContext::Expected(StrContextValue::Description(reason))
}

fn exposure_tsv(input: &mut &str) -> ModalResult<ExposureData> {
    let format = || {
        alt((
            alphanumeric1.map(|m| Some(String::from(m))),
            empty.value(None),
        ))
    };

    let aperture = || {
        alt((
            (opt("f"), float).map(|(_, m): (Option<&str>, f32)| Some(format!("{m}"))),
            empty.value(None),
        ))
    };

    seq! {ExposureData {
        sspeed: format().context(expected("sspeed")),
        _: tab,
        aperture: aperture().context(expected("aperture")),
        _: tab,
        lens: format().context(expected("lens")),
        _: tab,
        comment: take_till(0.., |c| c == '\t')
            .map(|m| Some(String::from(m)))
            .context(expected("comment")),
        _: tab,
        date: take_till(0.., |c| c == '\t')
            .map(|s: &str| {
                NaiveDateTime::parse_from_str(s, TIMESTAMP_FORMAT).ok()
            })
            .context(expected("date")),
        _: tab,
        gps: alt((
                separated_pair(float, ", ", float).map(Some),
                empty.value(None)
            )),
        ..Default::default()
    }}
    .parse_next(input)
}

fn encode_jpeg(image: &PathBuf, data: &ExposureData) -> Result<(), Box<dyn Error>> {
    let photo = ImageReader::open(&image)?
        .with_guessed_format()?
        .decode()?
        .resize(2000, 2000, image::imageops::FilterType::Lanczos3);

    let buffer_name = image.with_extension("jpeg-exifed");
    let buffer = File::create(&buffer_name)?;

    let mut f = io::Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut f)?;

    data.encode_exif(&mut encoder)?;

    let jpg_encoder = JpegEncoder::new_with_quality(buffer, 90).with_exif_metadata(f.into_inner());

    match format(photo) {
        SupportedImage::RGB(photo) => {
            jpg_encoder.write_image(
                &photo,
                photo.height(),
                photo.width(),
                image::ColorType::Rgb8.into(),
            )?;
        }
        SupportedImage::Gray(photo) => {
            jpg_encoder.write_image(
                &photo,
                photo.height(),
                photo.width(),
                image::ColorType::L8.into(),
            )?;
        }
    }

    std::fs::rename(&buffer_name, image.with_extension("jpg"))?;
    Ok(())
}

fn encode_tiff(image: &PathBuf, data: &ExposureData) -> Result<(), Box<dyn Error>> {
    let photo = ImageReader::open(&image)?.with_guessed_format()?.decode()?;

    let buffer_name = image.with_extension("tiff-exifed");
    let buffer = File::create(&buffer_name)?;

    let mut encoder = TiffEncoder::new(buffer)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    data.encode_exif(&mut encoder)?;

    match format(photo) {
        SupportedImage::RGB(photo) => {
            encoder.write_image::<colortype::RGB8>(photo.width(), photo.height(), &photo)?;
        }
        SupportedImage::Gray(photo) => {
            encoder.write_image::<colortype::Gray8>(photo.width(), photo.height(), &photo)?;
        }
    }

    std::fs::rename(&buffer_name, image.with_extension("tiff"))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    let level = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    SimpleLogger::new().with_level(level).init()?;

    let mut tse_file_path = args.dir.clone();
    tse_file_path.push("index.tse");

    let (exposures, meta): (Vec<_>, Vec<_>) = std::fs::read_to_string(tse_file_path)?
        .split('\n')
        .map(String::from)
        .partition(|line| !(line.starts_with('#') || line.starts_with(';')));

    let template = meta.iter().filter(|l| l.starts_with('#')).fold(
        ExposureData::default(),
        |mut acc, line| {
            let space = line.find(' ').unwrap();
            let (marker, value) = line[1..].split_at(space - 1);

            match marker {
                "Make" => acc.make = Some(value.trim().into()),
                "Model" => acc.model = Some(value.trim().into()),
                "Description" => acc.description = Some(value.trim().into()),
                "Author" => acc.author = Some(value.trim().into()),
                "ISO" => acc.iso = Some(value.trim().into()),
                &_ => (),
            }

            acc
        },
    );

    let mut exposures: Vec<(u32, ExposureData)> = (1..)
        .zip(exposures.into_iter())
        .map(
            |(index, line): (u32, String)| -> Result<Option<(u32, ExposureData)>, String> {
                match exposure_tsv(&mut line.as_str()) {
                    Ok(data) => Ok(Some((index, data.complete(&template)))),
                    Err(_) if line.is_empty() => Ok(None),
                    Err(e) => Err(format!("Failed to parse line {index}: {e} `{line}`")),
                }
            },
        )
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .flatten()
        .collect();

    exposures.sort_by(|lhs, rhs| lhs.0.partial_cmp(&rhs.0).unwrap());
    let mut last = None;

    let exposures: HashMap<u32, ExposureData> = exposures
        .into_iter()
        .map(|(i, mut data)| {
            match (data.date, last) {
                (Some(timestamp), None) => last = Some((timestamp, 1)),
                (Some(timestamp), Some((date, offset))) if timestamp == date => {
                    let step = DateTime::from_timestamp(date.and_utc().timestamp() + offset, 0)
                        .unwrap()
                        .naive_utc();
                    last = Some((date, offset + 1));
                    data.date = Some(step);
                }
                (Some(timestamp), Some(_)) => last = Some((timestamp, 1)),
                _ => (),
            };
            (i, data)
        })
        .collect();

    let mut files = HashMap::<u32, Vec<PathBuf>>::new();
    let re = Regex::new(r"([0-9]+)").unwrap();

    let out = std::fs::read_dir(args.dir)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    out.into_iter()
        .filter(|p| p.file_name().is_some())
        .filter_map(|p: PathBuf| {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .unwrap();
            match re.captures(&name) {
                Some(value) => {
                    let (index, _) = value.extract::<1>();
                    Some((p, String::from(index)))
                }
                None => None,
            }
        })
        .map(|(p, index_str)| {
            let path = p;
            str::parse::<u32>(&index_str).map(|i| match files.get_mut(&i) {
                Some(container) => container.push(path),
                None => {
                    files.insert(i, vec![path]);
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    for exposure_index in files.keys() {
        if !exposures.contains_key(exposure_index) {
            log::error!("No exposure data found for exposure number {exposure_index}");
        }
    }

    for (index, data) in exposures {
        let targets = files.get(&index);

        if targets.is_none() {
            log::error!("No files found for index {index}");
            break;
        }

        let _errs = targets
            .unwrap()
            .iter()
            .flat_map(|file: &PathBuf| vec![encode_jpeg(file, &data), encode_tiff(file, &data)])
            .filter(|r| r.is_err());
    }

    Ok(())
}
