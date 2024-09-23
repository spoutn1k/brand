use clap::Parser;
use image::ImageReader;
use simple_logger::SimpleLogger;
use std::error::Error;
use std::fs::File;

use tiff::encoder::{colortype, Compression, Predictor, TiffEncoder};

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

fn create_thumbnail(tiff: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let photo = ImageReader::open(tiff)?
        .with_guessed_format()?
        .decode()?
        .resize(2000, 2000, image::imageops::FilterType::Nearest);

    let thumbnail = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(tiff.with_extension("jpg"))?;

    image::codecs::jpeg::JpegEncoder::new(thumbnail).encode_image(&photo)?;

    Ok(())
}

fn compress(tiff: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let photo = ImageReader::open(tiff)?.with_guessed_format()?.decode()?;

    let compressed = File::create(tiff.with_extension("tiff"))?;

    let mut encoder = TiffEncoder::new(compressed)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    encoder.write_image::<colortype::RGB8>(
        photo.width(),
        photo.height(),
        photo.as_rgb8().expect("Wrong image format"),
    )?;

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    if args.debug {
        SimpleLogger::new()
            .with_level(log::LevelFilter::Debug)
            .init()?;
    } else {
        SimpleLogger::new()
            .with_level(log::LevelFilter::Trace)
            .init()?;
    }

    let out = std::fs::read_dir(args.dir)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;

    for file in out {
        match file.extension() {
            Some(e) if e == "tif" => {
                compress(&file)?;
                create_thumbnail(&file)?;
            }
            Some(e) if e == "tiff" => create_thumbnail(&file)?,
            _ => (),
        }
    }

    Ok(())
}
