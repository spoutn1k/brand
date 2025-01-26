use image::{DynamicImage, GrayImage, RgbImage};

pub enum SupportedImage {
    RGB(RgbImage),
    Gray(GrayImage),
}

impl Into<DynamicImage> for SupportedImage {
    fn into(self) -> DynamicImage {
        match self {
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
