use brand::image_management::{format, SupportedImage};
use clap::Parser;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageReader};
use regex::Regex;
use simple_logger::SimpleLogger;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use tiff::encoder::{colortype, Compression, Predictor, TiffEncoder};

/// Split a half-frame image into two halves, creating a compressed tiff image and a thumbnail jpg.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the exposure folder
    #[arg(required = true)]
    #[clap(default_value = ".")]
    dir: std::path::PathBuf,

    /// Turn debugging information on
    #[arg(short, long)]
    #[clap(default_value = "false")]
    debug: bool,
}

fn process(photo: &DynamicImage, path: &Path) -> Result<(), Box<dyn Error>> {
    compress(photo.clone(), &path.with_extension("tiff"))?;
    <SupportedImage as Into<DynamicImage>>::into(format(photo.clone()))
        .resize(2000, 2000, FilterType::Nearest)
        .save(path.with_extension("jpg"))?;

    Ok(())
}

fn compress(photo: DynamicImage, destination: &Path) -> Result<(), Box<dyn Error>> {
    log::debug!(
        "Compressing image to {} ({:?})",
        destination.display(),
        photo.color()
    );

    let compressed = File::create(destination)?;

    let mut encoder = TiffEncoder::new(compressed)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    match format(photo) {
        SupportedImage::RGB(photo) => {
            encoder.write_image::<colortype::RGB8>(photo.width(), photo.height(), &photo)?
        }

        SupportedImage::Gray(photo) => {
            encoder.write_image::<colortype::Gray8>(photo.width(), photo.height(), &photo)?
        }
    }

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

    let out = std::fs::read_dir(args.dir)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;

    let filenameformat = Regex::new(r"(\d+).tif$")?;

    for file in out {
        let filename = file
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();

        if let Some(index) = filenameformat.captures(&filename) {
            log::debug!("Processing file `{}`", file.display());
            let index = index[1].parse::<u32>()?;

            let photo = ImageReader::open(&file)?.with_guessed_format()?.decode()?;
            let split = (photo.height() as f32 * 17f32 / 24f32).round() as u32;

            process(
                &photo.view(0, 0, split, photo.height()).to_image().into(),
                &file.with_file_name(format!("{}", index * 2 - 1)),
            )?;

            process(
                &photo
                    .view(photo.width() - split, 0, split, photo.height())
                    .to_image()
                    .into(),
                &file.with_file_name(format!("{}", index * 2)),
            )?;
        }
    }

    Ok(())
}
