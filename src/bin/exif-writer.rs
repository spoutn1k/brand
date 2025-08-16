use brand::{
    image_management::{encode_jpeg_with_exif, encode_tiff_with_exif},
    models::ExposureData,
};
use chrono::{DateTime, NaiveDateTime};
use clap::Parser;
use image::ImageReader;
use regex::Regex;
use simple_logger::SimpleLogger;
use std::{collections::HashMap, error::Error, fs::File, path::PathBuf};
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

    encode_jpeg_with_exif(photo, buffer, data)?;

    std::fs::rename(&buffer_name, image.with_extension("jpg"))?;
    Ok(())
}

fn encode_tiff(image: &PathBuf, data: &ExposureData) -> Result<(), Box<dyn Error>> {
    let photo = ImageReader::open(&image)?.with_guessed_format()?.decode()?;

    let buffer_name = image.with_extension("tiff-exifed");
    let buffer = File::create(&buffer_name)?;

    encode_tiff_with_exif(photo, buffer, data)?;

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
