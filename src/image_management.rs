use crate::{
    Error,
    gps::{GpsRef, GpsTag},
    models::ExposureData,
};
use image::{DynamicImage, GrayImage, ImageEncoder, RgbImage, codecs::jpeg::JpegEncoder};
use std::io::{Cursor, Seek, Write};
use tiff::{
    encoder::{
        Compression, DirectoryEncoder, DirectoryOffset, Predictor, TiffEncoder, TiffKindStandard,
        colortype,
    },
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
) -> Result<(), Error> {
    let mut f = Vec::new();
    let mut encoder = TiffEncoder::new(Cursor::new(&mut f))?;

    let exif_data = encode_exif_ifd(data, &mut encoder)?;
    let gps_data = encode_gps_ifd(data, &mut encoder)?;
    let mut main = encoder.image_directory()?;

    encode_exif(data, &mut main)?;
    main.write_tag(Tag::ExifDirectory, exif_data.offset)?;
    if let Some(gps_offset) = gps_data {
        main.write_tag(Tag::GpsDirectory, gps_offset.offset)?;
    }
    main.finish()?;

    let mut jpg_encoder = JpegEncoder::new_with_quality(output, 90);
    jpg_encoder.set_exif_metadata(f)?;

    match format(input) {
        SupportedImage::RGB(photo) => {
            jpg_encoder.write_image(
                &photo,
                photo.width(),
                photo.height(),
                image::ColorType::Rgb8.into(),
            )?;
        }
        SupportedImage::Gray(photo) => {
            jpg_encoder.write_image(
                &photo,
                photo.width(),
                photo.height(),
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
) -> Result<(), Error> {
    let mut encoder = TiffEncoder::new(output)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);

    let exif_data = encode_exif_ifd(data, &mut encoder)?;
    let gps_data = encode_gps_ifd(data, &mut encoder)?;

    match format(input) {
        SupportedImage::RGB(photo) => {
            let mut image = encoder.new_image::<colortype::RGB8>(photo.width(), photo.height())?;
            encode_exif(data, image.encoder())?;
            image
                .encoder()
                .write_tag(Tag::ExifDirectory, exif_data.offset)?;
            if let Some(gps_offset) = gps_data {
                image
                    .encoder()
                    .write_tag(Tag::GpsDirectory, gps_offset.offset)?;
            }
            image.write_data(&photo)?;
        }
        SupportedImage::Gray(photo) => {
            let mut image = encoder.new_image::<colortype::Gray8>(photo.width(), photo.height())?;
            encode_exif(data, image.encoder())?;
            image
                .encoder()
                .write_tag(Tag::ExifDirectory, exif_data.offset)?;
            if let Some(gps_offset) = gps_data {
                image
                    .encoder()
                    .write_tag(Tag::GpsDirectory, gps_offset.offset)?;
            }
            image.write_data(&photo)?;
        }
    }

    Ok(())
}

pub fn encode_exif<'a, W: Write + Seek>(
    data: &'a ExposureData,
    encoder: &mut DirectoryEncoder<'a, W, TiffKindStandard>,
) -> Result<(), Error> {
    if let Some(value) = &data.author {
        encoder.write_tag(Tag::Artist, value.as_str())?;
    }

    if let Some(make) = &data.make {
        encoder.write_tag(Tag::Make, make.as_str())?;
    }

    if let Some(model) = &data.model {
        encoder.write_tag(Tag::Model, model.as_str())?;
    }

    match (&data.description, &data.comment) {
        (Some(description), Some(comment)) => {
            encoder.write_tag(
                Tag::ImageDescription,
                format!("{description} - {comment}").as_str(),
            )?;
        }
        (Some(description), None) => {
            encoder.write_tag(Tag::ImageDescription, description.as_str())?;
        }
        (None, Some(comment)) => {
            encoder.write_tag(Tag::ImageDescription, comment.as_str())?;
        }
        _ => (),
    }

    if let Some(date) = &data.date {
        encoder.write_tag(
            Tag::DateTime,
            date.format("%Y:%m:%d %H:%M:%S").to_string().as_str(),
        )?;
    }

    Ok(())
}

pub fn encode_exif_ifd<W: Write + Seek>(
    data: &ExposureData,
    encoder: &mut TiffEncoder<W>,
) -> Result<DirectoryOffset<TiffKindStandard>, Error> {
    let mut dir = encoder.extra_directory()?;

    if let Some(iso) = &data.iso {
        let iso = iso.parse::<u16>().unwrap();
        dir.write_tag(Tag::Unknown(0x8827), iso)?;
    }

    if let Some(shutter_speed) = data.sspeed.as_ref().and_then(|s| s.parse::<u16>().ok()) {
        dir.write_tag(Tag::Unknown(0x9201), shutter_speed)?;
    }

    if let Some(date) = &data.date {
        dir.write_tag(
            Tag::Unknown(0x9003),
            date.format("%Y:%m:%d %H:%M:%S").to_string().as_str(),
        )?;
    }

    Ok(dir.finish_with_offsets()?)
}

pub fn encode_gps_ifd<W: Write + Seek>(
    data: &ExposureData,
    encoder: &mut TiffEncoder<W>,
) -> Result<Option<DirectoryOffset<TiffKindStandard>>, Error> {
    Ok(data
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
        .transpose()?)
}
