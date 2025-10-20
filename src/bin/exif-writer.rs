use brand::{
    Error, analyze_files,
    image_management::{encode_jpeg_with_exif, encode_tiff_with_exif},
    models::{Data, ExposureData, read_tse},
};
use clap::Parser;
use image::ImageReader;
use simple_logger::SimpleLogger;
use std::{fs::File, path::PathBuf};

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

    let directory_contents = std::fs::read_dir(args.dir)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;

    let (images, _) = analyze_files(&directory_contents)?;

    let data = Data {
        files: images.iter().map(|i| i.0.clone()).collect(),
        ..Default::default()
    };

    let exif_data = read_tse(data, std::fs::read_to_string(tse_file_path)?.as_bytes())?;

    for (meta, path) in images {
        if exif_data.get_exposure(meta.index).is_none() {
            log::error!("No exposure data found for exposure number {}", meta.index);
        }

        encode_jpeg(path, &exif_data.generate(meta.index))?;
        encode_tiff(path, &exif_data.generate(meta.index))?;
    }

    Ok(())
}
