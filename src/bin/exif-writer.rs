use brand::{
    Error,
    image_management::{encode_jpeg_with_exif, encode_tiff_with_exif},
    models::{ExposureData, read_tse},
};
use clap::Parser;
use image::ImageReader;
use regex::Regex;
use simple_logger::SimpleLogger;
use std::{collections::HashMap, fs::File, path::PathBuf};

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

fn encode_jpeg(image: &PathBuf, data: &ExposureData) -> Result<(), Error> {
    let photo = ImageReader::open(image)?
        .with_guessed_format()?
        .decode()?
        .resize(2000, 2000, image::imageops::FilterType::Nearest);

    let buffer_name = image.with_extension("jpeg-exifed");
    let buffer = File::create(&buffer_name)?;

    encode_jpeg_with_exif(photo, buffer, data)?;

    std::fs::rename(&buffer_name, image.with_extension("jpg"))?;
    Ok(())
}

fn encode_tiff(image: &PathBuf, data: &ExposureData) -> Result<(), Error> {
    let photo = ImageReader::open(image)?.with_guessed_format()?.decode()?;

    let buffer_name = image.with_extension("tiff-exifed");
    let buffer = File::create(&buffer_name)?;

    encode_tiff_with_exif(photo, buffer, data)?;

    std::fs::rename(&buffer_name, image.with_extension("tiff"))?;
    Ok(())
}

fn main() -> Result<(), Error> {
    let args = Cli::parse();

    let level = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    SimpleLogger::new().with_level(level).init()?;

    let mut tse_file_path = args.dir.clone();
    tse_file_path.push("index.tse");

    let exif_data = read_tse(std::fs::read_to_string(tse_file_path)?.as_bytes())?;

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
        if !exif_data.exposures.contains_key(exposure_index) {
            log::error!("No exposure data found for exposure number {exposure_index}");
        }
    }

    for index in exif_data.exposures.keys() {
        let targets = files.get(index);

        if targets.is_none() {
            log::error!("No files found for index {index}");
            break;
        }

        let _errs = targets
            .unwrap()
            .iter()
            .flat_map(|file: &PathBuf| {
                vec![
                    encode_jpeg(file, &exif_data.generate(*index)),
                    encode_tiff(file, &exif_data.generate(*index)),
                ]
            })
            .filter(|r| r.is_err())
            .collect::<Vec<_>>();
    }

    Ok(())
}
