use brand::image_management::{format, SupportedImage};
use clap::Parser;
use image::{DynamicImage, ImageReader};
use simple_logger::SimpleLogger;
use std::error::Error;
use std::fs::File;
use tiff::encoder::{colortype, Compression, Predictor, TiffEncoder};

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

fn compress(photo: &DynamicImage, destination: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let mut encoder = TiffEncoder::new(File::create(destination)?)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    match format(photo.clone()) {
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

    for file in out {
        match file.clone().extension().map(|e| e.to_string_lossy()) {
            Some(e) if e.starts_with("tif") => {
                let photo = ImageReader::open(&file)?.with_guessed_format()?.decode()?;
                if e == "tif" {
                    compress(&photo, &file.with_extension("tiff"))?;
                }

                photo
                    .resize(2000, 2000, image::imageops::FilterType::Nearest)
                    .save(file.with_extension("jpg"))?;
            }
            _ => (),
        }
    }

    Ok(())
}
