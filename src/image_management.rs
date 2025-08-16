use crate::{
    gps::{GpsRef, GpsTag},
    models::ExposureData,
};
use image::{
    DynamicImage, GrayImage, ImageEncoder, ImageReader, RgbImage, codecs::jpeg::JpegEncoder,
};
use std::{
    error::Error,
    io::{Cursor, Seek, Write},
};
use tiff::{
    encoder::{Compression, Predictor, Rational, TiffEncoder, colortype},
    tags::Tag,
};

pub enum SupportedImage {
    RGB(RgbImage),
    Gray(GrayImage),
}

impl From<SupportedImage> for DynamicImage {
    fn from(val: SupportedImage) -> DynamicImage {
        match val {
            SupportedImage::RGB(image) => DynamicImage::ImageRgb8(image),
            SupportedImage::Gray(image) => DynamicImage::ImageLuma8(image),
        }
    }
}

pub fn format(image: DynamicImage) -> SupportedImage {
    match image {
        DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => SupportedImage::RGB(image.into_rgb8()),
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_) => SupportedImage::Gray(image.into_luma8()),
        _ => panic!("Unsupported image type: {:?}", image.color()),
    }
}

pub fn encode_jpeg_with_exif<O: Write>(
    input: DynamicImage,
    output: O,
    data: &ExposureData,
) -> Result<(), Box<dyn Error>> {
    let mut f = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut f)?;

    encode_exif(data, &mut encoder)?;

    let jpg_encoder = JpegEncoder::new_with_quality(output, 90).with_exif_metadata(f.into_inner());

    match format(input) {
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

    Ok(())
}

pub fn encode_tiff_with_exif<O: Write + Seek>(
    input: DynamicImage,
    output: O,
    data: &ExposureData,
) -> Result<(), Box<dyn Error>> {
    let mut encoder = TiffEncoder::new(output)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    encode_exif(data, &mut encoder)?;

    match format(input) {
        SupportedImage::RGB(photo) => {
            encoder.write_image::<colortype::RGB8>(photo.width(), photo.height(), &photo)?;
        }
        SupportedImage::Gray(photo) => {
            encoder.write_image::<colortype::Gray8>(photo.width(), photo.height(), &photo)?;
        }
    }

    Ok(())
}

pub fn encode_exif<W: Write + Seek>(
    data: &ExposureData,
    encoder: &mut TiffEncoder<W>,
) -> Result<(), tiff::TiffError> {
    let gps_off = &data
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

    if let Some(value) = &data.author {
        dir.write_tag(Tag::Artist, value.as_str())?;
    }

    if let Some(make) = &data.make {
        dir.write_tag(Tag::Make, make.as_str())?;
    }

    if let Some(model) = &data.model {
        dir.write_tag(Tag::Model, model.as_str())?;
    }

    match (&data.description, &data.comment) {
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

    if let Some(iso) = &data.iso {
        let iso = iso.parse::<u16>().unwrap();
        dir.write_tag(Tag::Unknown(0x8827), iso)?;
    }

    if let Some(shutter_speed) = &data.sspeed {
        dir.write_tag(Tag::Unknown(0x9201), shutter_speed.parse::<u16>().unwrap())?;
    }

    dir.finish()
}
