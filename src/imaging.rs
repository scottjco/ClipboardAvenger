use image::{DynamicImage, ImageEncoder};

pub fn dib_image(b: &[u8]) -> Option<DynamicImage> {
    if b.len() < 40 {
        return None;
    }
    let u = |o| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let header = u(0) as usize;
    if header < 40 || header > b.len() {
        return None;
    }
    let bits = u16::from_le_bytes(b[14..16].try_into().unwrap());
    let compression = u(16);
    let colors = u(32) as usize;
    let masks = if header == 40 && compression == 3 {
        12
    } else if header == 40 && compression == 6 {
        16
    } else {
        0
    };
    let palette = if colors > 0 {
        colors.checked_mul(4)?
    } else if bits <= 8 {
        (1usize << bits) * 4
    } else {
        0
    };
    let off = 14usize
        .checked_add(header)?
        .checked_add(masks)?
        .checked_add(palette)?;
    let mut bmp = Vec::with_capacity(b.len() + 14);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((b.len() + 14) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&(off as u32).to_le_bytes());
    bmp.extend_from_slice(b);
    image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()
}

pub struct Encoded {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

fn scaled_dimension(dimension: u32, percent: u8) -> u32 {
    ((u64::from(dimension) * u64::from(percent) + 50) / 100).max(1) as u32
}

fn has_transparency(img: &DynamicImage) -> bool {
    match img {
        DynamicImage::ImageLumaA8(im) => im.pixels().any(|p| p[1] != u8::MAX),
        DynamicImage::ImageRgba8(im) => im.pixels().any(|p| p[3] != u8::MAX),
        DynamicImage::ImageLumaA16(im) => im.pixels().any(|p| p[1] != u16::MAX),
        DynamicImage::ImageRgba16(im) => im.pixels().any(|p| p[3] != u16::MAX),
        DynamicImage::ImageRgba32F(im) => im.pixels().any(|p| p[3] < 1.0),
        _ => false,
    }
}

fn resize_transparent(img: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    let high_depth = matches!(
        img,
        DynamicImage::ImageLumaA16(_)
            | DynamicImage::ImageRgba16(_)
            | DynamicImage::ImageRgba32F(_)
    );
    if (img.width(), img.height()) == (width, height) {
        return if high_depth {
            DynamicImage::ImageRgba16(img.to_rgba16())
        } else {
            DynamicImage::ImageRgba8(img.to_rgba8())
        };
    }
    // image::imageops::resize expects premultiplied alpha. Doing this in float
    // avoids both dark fringes and the rounding loss of premultiplied 8-bit RGB.
    let mut premultiplied = img.to_rgba32f();
    for p in premultiplied.pixels_mut() {
        for channel in 0..3 {
            p[channel] *= p[3];
        }
    }
    let mut resized = image::imageops::resize(
        &premultiplied,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    for p in resized.pixels_mut() {
        let alpha = p[3].clamp(0.0, 1.0);
        for channel in 0..3 {
            p[channel] = if alpha > 0.0 {
                (p[channel] / alpha).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        p[3] = alpha;
    }
    let resized = DynamicImage::ImageRgba32F(resized);
    if high_depth {
        DynamicImage::ImageRgba16(resized.to_rgba16())
    } else {
        DynamicImage::ImageRgba8(resized.to_rgba8())
    }
}

pub fn encoded(
    img: &DynamicImage,
    original: usize,
    quality: u8,
    resize_percent: u8,
) -> Option<Encoded> {
    if !matches!(resize_percent, 25 | 50 | 75 | 100) || img.width() == 0 || img.height() == 0 {
        return None;
    }
    // Inspect the source, because downsampling can round a single faintly
    // transparent pixel to opaque. Such inputs must still remain PNG.
    let transparent = has_transparency(img);
    let (width, height) = (
        scaled_dimension(img.width(), resize_percent),
        scaled_dimension(img.height(), resize_percent),
    );
    let mut out = Vec::new();
    let extension = if transparent {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        let rgba = resize_transparent(img, width, height);
        PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive)
            .write_image(rgba.as_bytes(), width, height, rgba.color().into())
            .ok()?;
        "png"
    } else {
        let mut rgb = img.to_rgb8();
        if rgb.dimensions() != (width, height) {
            rgb =
                image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Triangle);
        }
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .ok()?;
        "jpg"
    };
    (out.len() < original).then_some(Encoded {
        bytes: out,
        extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn every_resize_choice_uses_rounded_dimensions() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            101,
            53,
            image::Rgb([80, 140, 220]),
        ));
        for (percent, dimensions) in [
            (100, (101, 53)),
            (75, (76, 40)),
            (50, (51, 27)),
            (25, (25, 13)),
        ] {
            let out = encoded(&img, usize::MAX, 95, percent).unwrap();
            assert_eq!(out.extension, "jpg");
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), dimensions);
            assert!(encoded(&img, out.bytes.len(), 95, percent).is_none());
        }
    }

    #[test]
    fn resize_clamps_tiny_images_to_one_pixel() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            1,
            2,
            image::Rgb([80, 140, 220]),
        ));
        for percent in [25, 50] {
            let out = encoded(&img, usize::MAX, 95, percent).unwrap();
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (1, 1));
        }
    }

    #[test]
    fn transparent_source_remains_png_after_downsampling() {
        let mut rgba = RgbaImage::from_pixel(100, 100, image::Rgba([27, 110, 42, 255]));
        rgba.put_pixel(12, 34, image::Rgba([27, 110, 42, 254]));
        let img = DynamicImage::ImageRgba8(rgba);
        for percent in [25, 50, 75] {
            let out = encoded(&img, usize::MAX, 95, percent).unwrap();
            assert_eq!(out.extension, "png");
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!(
                (decoded.width(), decoded.height()),
                (u32::from(percent), u32::from(percent))
            );
        }
    }

    #[test]
    fn no_resize_preserves_every_transparent_pixel_exactly() {
        let rgba = RgbaImage::from_fn(31, 17, |x, y| {
            image::Rgba([(x * 8) as u8, (y * 14) as u8, 150, (x * y % 256) as u8])
        });
        let img = DynamicImage::ImageRgba8(rgba.clone());
        for quality in [50, 95, 100] {
            let out = encoded(&img, usize::MAX, quality, 100).unwrap();
            assert_eq!(out.extension, "png");
            assert_eq!(
                image::load_from_memory(&out.bytes).unwrap().to_rgba8(),
                rgba
            );
        }
    }

    #[test]
    fn transparent_edges_do_not_gain_dark_halos() {
        let mut rgba = RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 0]));
        rgba.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        let out = encoded(&DynamicImage::ImageRgba8(rgba), usize::MAX, 95, 50).unwrap();
        let decoded = image::load_from_memory(&out.bytes).unwrap().to_rgba8();
        assert_eq!(&decoded.get_pixel(0, 0).0[..3], &[255, 255, 255]);
        assert!((1..255).contains(&decoded.get_pixel(0, 0)[3]));
    }

    #[test]
    fn any_transparency_is_detected_before_bit_depth_conversion() {
        let img = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            10,
            10,
            image::Rgba([20000, 30000, 40000, 65534]),
        ));
        for percent in [25, 50, 75, 100] {
            let out = encoded(&img, usize::MAX, 95, percent).unwrap();
            assert_eq!(out.extension, "png");
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!(decoded.color(), image::ColorType::Rgba16);
            assert!(decoded.to_rgba16().pixels().all(|p| p[3] == 65534));
            if percent == 100 {
                assert_eq!(decoded.to_rgba16(), img.to_rgba16());
            }
        }
    }
}
