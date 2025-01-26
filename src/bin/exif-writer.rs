use chrono::NaiveDateTime;
use clap::Parser;
use regex::Regex;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use simple_logger::SimpleLogger;
use std::error::Error;
use winnow::{
    ascii::{alphanumeric1, float, tab},
    combinator::{alt, empty, opt, separated_pair, seq},
    error::{StrContext, StrContextValue},
    token::take_till,
    PResult, Parser as _,
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
            date: self.date.or(other.date.clone()),
            gps: self.gps.or(other.gps.clone()),
        }
    }
}

impl Serialize for ExposureData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("ExposureData", 3)?;
        if let Some(author) = &self.author {
            s.serialize_field("Author", &author)?;
            s.serialize_field("Artist", &author)?;
        }
        if let Some(make) = &self.make {
            s.serialize_field("Make", &make)?;
        }
        if let Some(model) = &self.model {
            s.serialize_field("Model", &model)?;
        }
        if let Some(comment) = &self.comment {
            let description: String;
            if let Some(film) = &self.description {
                description = format!("{comment} - {film}");
            } else {
                description = format!("{comment} - Unknown film");
            }

            s.serialize_field("ImageDescription", &description)?;
            s.serialize_field("Description", &description)?;
        }
        if let Some(timestamp) = &self.date {
            s.serialize_field("alldates", &timestamp.format(TIMESTAMP_FORMAT).to_string())?;
        }
        if let Some(lens) = &self.lens {
            s.serialize_field("focallength", &lens)?;
        }
        if let Some(iso) = &self.iso {
            s.serialize_field("ISO", &iso)?;
        }
        if let Some(aperture) = &self.aperture {
            s.serialize_field("FNumber", &aperture)?;
            s.serialize_field("ApertureValue", &aperture)?;
        }
        if let Some(sspeed) = &self.sspeed {
            s.serialize_field("shutterspeed", &sspeed)?;
        }

        if let Some(coords) = &self.gps {
            let (lat, lon): (f64, f64);
            if coords.0 < 0.0 {
                s.serialize_field("GPSLatitudeRef", "S")?;
                lat = -coords.0;
            } else {
                s.serialize_field("GPSLatitudeRef", "N")?;
                lat = coords.0;
            }
            if coords.1 < 0.0 {
                s.serialize_field("GPSLongitudeRef", "W")?;
                lon = -coords.1;
            } else {
                s.serialize_field("GPSLongitudeRef", "E")?;
                lon = coords.1;
            }

            s.serialize_field("GPSLatitude", &format!("{lat}"))?;
            s.serialize_field("GPSLongitude", &format!("{lon}"))?;
        }
        s.end()
    }
}

pub fn expected(reason: &'static str) -> StrContext {
    StrContext::Expected(StrContextValue::Description(reason))
}

fn exposure_tsv(input: &mut &str) -> PResult<ExposureData> {
    let format = || {
        alt((
            alphanumeric1.map(|m| Some(String::from(m))),
            empty.value(None),
        ))
    };

    let aperture = || {
        alt((
            (opt("f"), float).map(|(_, m): (std::option::Option<&str>, f32)| Some(format!("{m}"))),
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
        // default initialization also works
        ..Default::default()
    }}
    .parse_next(input)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    if args.debug {
        SimpleLogger::new()
            .with_level(log::LevelFilter::Debug)
            .init()?;
    } else {
        SimpleLogger::new()
            .with_level(log::LevelFilter::Info)
            .init()?;
    }

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

    log::debug!("Default fields: {}", serde_json::to_string(&template)?);

    let mut exposures: Vec<(u32, ExposureData)> = (1..)
        .zip(exposures.into_iter())
        .map(
            |(index, line): (u32, String)| -> Result<Option<(u32, ExposureData)>, String> {
                match exposure_tsv(&mut line.as_str()) {
                    Ok(data) => Ok(Some((index as u32, data.complete(&template)))),
                    Err(_) if line.is_empty() => Ok(None),
                    Err(e) => Err(format!("Failed to parse line {index}: {e} `{line}`")),
                }
            },
        )
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .filter_map(|e| e)
        .collect();

    exposures.sort_by(|lhs, rhs| lhs.0.partial_cmp(&rhs.0).unwrap());
    let mut last = None;

    use chrono::DateTime;
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
            str::parse::<u32>(&index_str).and_then(|i| match files.get_mut(&i) {
                Some(container) => Ok(container.push(path)),
                None => {
                    files.insert(i, vec![path]);
                    Ok(())
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    for exposure_index in files.keys() {
        if exposures.get(exposure_index).is_none() {
            log::error!("No exposure data found for exposure number {exposure_index}");
        }
    }

    for (index, data) in exposures {
        let targets = files.get(&index);

        if targets.is_none() {
            log::error!("No files found for index {index}");
            break;
        }

        let hash = format!("photo-{index}");
        let mut dump_file = std::env::temp_dir();
        dump_file.push(format!("{hash}.json"));
        let dump = std::fs::File::create(&dump_file)?;
        let dump_file = dump_file.as_os_str().to_str().unwrap();
        serde_json::to_writer(dump, &data)?;

        targets
            .unwrap()
            .iter()
            .map(|f: &String| {
                let mut tagging = std::process::Command::new("exiftool");
                tagging.args([
                    "-m",
                    "-q",
                    "-overwrite_original",
                    &format!("-j={dump_file}"),
                    &f,
                ]);

                log::debug!("{tagging:?}");
                tagging.spawn()
            })
            .collect::<Result<Vec<_>, _>>()?;
    }

    Ok(())
}
