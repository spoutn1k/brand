use brand::image_management::{format, SupportedImage};
use chrono::{DateTime, NaiveDateTime};
use clap::Parser;
use image::ImageReader;
use regex::Regex;
use simple_logger::SimpleLogger;
use std::{error::Error, fs::File};
use tiff::{
    encoder::{colortype, Compression, Predictor, TiffEncoder},
    ifd::{Directory, ImageFileDirectory, ProcessedEntry, Value},
    tags::{GpsTag, Tag},
};
use winnow::{
    ascii::{alphanumeric1, float, tab},
    combinator::{alt, empty, opt, separated_pair, seq},
    error::{StrContext, StrContextValue},
    token::take_till,
    ModalResult, Parser as _,
};

static TIMESTAMP_FORMAT: &str = "%Y %m %d %H %M %S";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the exposure folder containing the index.tse file
    #[arg(required = true)]
    #[clap(default_value = ".")]
    dir: std::path::PathBuf,

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
    fn gps_ref(&self) -> ProcessedEntry;
}

impl GpsRef for f64 {
    /// Converts a GPS coordinate to the format expected by the EXIF standard
    fn gps_ref(&self) -> ProcessedEntry {
        let mut components = vec![
            Value::Rational(*self as u32, 1),
            Value::Rational((self.fract() * 60.0) as u32, 1),
        ];

        let sec_raw = (self * 60.0).fract() * 60.0;

        components.push(
            match num_rational::Ratio::<u32>::approximate_float_unsigned(sec_raw) {
                Some(sec) => Value::Rational(*sec.numer(), *sec.denom()),
                None => Value::Rational(sec_raw as u32, 1),
            },
        );

        ProcessedEntry::new_vec(&components)
    }
}

impl From<ExposureData> for ImageFileDirectory<GpsTag, ProcessedEntry> {
    fn from(e: ExposureData) -> Self {
        let mut out = Self::new();

        if let Some(coords) = &e.gps {
            let (lat, lon): (f64, f64);
            if coords.0 < 0.0 {
                out.insert(
                    GpsTag::GPSLatitudeRef,
                    ProcessedEntry::new(Value::Ascii("S".into())),
                );
                lat = -coords.0;
            } else {
                out.insert(
                    GpsTag::GPSLatitudeRef,
                    ProcessedEntry::new(Value::Ascii("N".into())),
                );
                lat = coords.0;
            }
            if coords.1 < 0.0 {
                out.insert(
                    GpsTag::GPSLongitudeRef,
                    ProcessedEntry::new(Value::Ascii("W".into())),
                );
                lon = -coords.1;
            } else {
                out.insert(
                    GpsTag::GPSLongitudeRef,
                    ProcessedEntry::new(Value::Ascii("E".into())),
                );
                lon = coords.1;
            }

            out.insert(GpsTag::GPSLatitude, lat.gps_ref());
            out.insert(GpsTag::GPSLongitude, lon.gps_ref());
        }

        out
    }
}

impl From<ExposureData> for Directory<ProcessedEntry> {
    fn from(e: ExposureData) -> Self {
        let mut out = Self::new();

        if let Some(value) = e.author {
            out.insert(Tag::Artist, ProcessedEntry::new(Value::Ascii(value)));
        }

        if let Some(make) = e.make {
            out.insert(Tag::Make, ProcessedEntry::new(Value::Ascii(make)));
        }

        if let Some(model) = e.model {
            out.insert(Tag::Model, ProcessedEntry::new(Value::Ascii(model)));
        }

        match (e.description, e.comment) {
            (Some(description), Some(comment)) => {
                out.insert(
                    Tag::ImageDescription,
                    ProcessedEntry::new(Value::Ascii(format!("{description} - {comment}"))),
                );
            }
            (Some(description), None) => {
                out.insert(
                    Tag::ImageDescription,
                    ProcessedEntry::new(Value::Ascii(description)),
                );
            }
            (None, Some(comment)) => {
                out.insert(
                    Tag::ImageDescription,
                    ProcessedEntry::new(Value::Ascii(comment)),
                );
            }
            _ => (),
        }

        if let Some(iso) = e.iso {
            out.insert(
                Tag::ISO,
                ProcessedEntry::new(Value::Short(iso.parse().unwrap())),
            );
        }

        if let Some(shutter_speed) = e.sspeed {
            out.insert(
                Tag::ShutterSpeedValue,
                ProcessedEntry::new(Value::Short(shutter_speed.parse().unwrap())),
            );
        }

        out
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

    let exposures: std::collections::HashMap<u32, ExposureData> = exposures
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

    let mut files = std::collections::HashMap::<u32, Vec<String>>::new();
    let re = Regex::new(r"([0-9]+)").unwrap();

    let out = std::fs::read_dir(args.dir)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    out.into_iter()
        .filter(|p| p.file_name().is_some())
        .filter_map(|p: std::path::PathBuf| {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .unwrap();
            match re.captures(&name) {
                Some(value) => {
                    let (index, _) = value.extract::<1>();
                    Some((
                        p.as_os_str().to_str().map(String::from).unwrap(),
                        String::from(index),
                    ))
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

        targets
            .unwrap()
            .iter()
            .map(|file: &String| -> Result<(), Box<dyn Error>> {
                let photo = ImageReader::open(&file)?.with_guessed_format()?.decode()?;

                let buffer_name = format!("{file}-exifed");
                let buffer = File::create(&buffer_name)?;

                let mut encoder = TiffEncoder::new(buffer)?
                    .with_compression(Compression::Lzw)
                    .with_predictor(Predictor::Horizontal)
                    .with_exif(data.clone().into())
                    .with_gps(data.clone().into());

                match format(photo.clone()) {
                    SupportedImage::RGB(photo) => encoder.write_image::<colortype::RGB8>(
                        photo.width(),
                        photo.height(),
                        &photo,
                    )?,

                    SupportedImage::Gray(photo) => encoder.write_image::<colortype::Gray8>(
                        photo.width(),
                        photo.height(),
                        &photo,
                    )?,
                }

                std::fs::rename(&buffer_name, file)?;

                Ok(())
            })
            .collect::<Result<Vec<_>, _>>()?;
    }

    Ok(())
}
