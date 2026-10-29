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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum Compression {
    Png = 0,
    #[default]
    Jpg = 1,
    Source = 3,
}
impl Compression {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Png,
            3 => Self::Source,
            _ => Self::Jpg,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpg => "JPG",
            Self::Source => "Same as Source",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        // Migrate the removed WebP output choice to the default without
        // disturbing any of the user's other saved settings.
        if text.eq_ignore_ascii_case("WEBP") {
            return Some(Self::Jpg);
        }
        [Self::Png, Self::Jpg, Self::Source]
            .into_iter()
            .find(|v| v.label().eq_ignore_ascii_case(text))
    }
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

#[cfg(test)]
pub fn encoded(
    img: &DynamicImage,
    original: usize,
    quality: u8,
    resize_percent: u8,
) -> Option<Encoded> {
    encoded_as(
        img,
        original,
        quality,
        resize_percent,
        Compression::Jpg,
        image::ImageFormat::Bmp,
        false,
        true,
    )
}

pub fn encoded_as(
    img: &DynamicImage,
    original: usize,
    quality: u8,
    resize_percent: u8,
    compression: Compression,
    source: image::ImageFormat,
    automatic_webp_to_png: bool,
    convert_opaque_to_jpg: bool,
) -> Option<Encoded> {
    if !matches!(resize_percent, 25 | 50 | 75 | 100) || img.width() == 0 || img.height() == 0 {
        return None;
    }
    // Inspect the source, because downsampling can round a single faintly
    // transparent pixel to opaque. Never discard source transparency.
    let transparent = has_transparency(img);
    let (width, height) = (
        scaled_dimension(img.width(), resize_percent),
        scaled_dimension(img.height(), resize_percent),
    );
    let convert_webp = automatic_webp_to_png && source == image::ImageFormat::WebP;
    let mut format = match compression {
        Compression::Png => image::ImageFormat::Png,
        Compression::Jpg => image::ImageFormat::Jpeg,
        Compression::Source => source,
    };
    // Turning off opaque conversion keeps PNG/WebP out of the JPEG encoder,
    // even when JPG is selected for bitmaps and other source formats.
    if compression == Compression::Jpg
        && !convert_opaque_to_jpg
        && matches!(source, image::ImageFormat::Png | image::ImageFormat::WebP)
    {
        format = source;
    }
    // The conversion checkbox takes priority over Same as Source. Opaque
    // WebP can use a smaller JPEG only when both JPG and opaque conversion
    // are selected. Otherwise automatic WebP conversion produces PNG.
    if convert_webp && (transparent || format != image::ImageFormat::Jpeg) {
        format = image::ImageFormat::Png;
    }
    if transparent
        && !matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::WebP
                | image::ImageFormat::Tiff
                | image::ImageFormat::Ico
        )
    {
        format = image::ImageFormat::Png;
    }
    // WebP stores 8-bit alpha. Keep faint 16-bit transparency intact in PNG.
    if transparent
        && format == image::ImageFormat::WebP
        && matches!(
            img,
            DynamicImage::ImageLumaA16(_)
                | DynamicImage::ImageRgba16(_)
                | DynamicImage::ImageRgba32F(_)
        )
    {
        format = image::ImageFormat::Png;
    }
    let output = encode_format(img, width, height, quality, format, transparent)?;
    if output.bytes.len() < original {
        return Some(output);
    }
    if convert_webp {
        // Format conversion is a compatibility feature: unlike normal
        // compression it must also work when PNG is larger than WebP.
        return if format == image::ImageFormat::Png {
            Some(output)
        } else {
            encode_format(
                img,
                width,
                height,
                quality,
                image::ImageFormat::Png,
                transparent,
            )
        };
    }
    None
}

fn encode_format(
    img: &DynamicImage,
    width: u32,
    height: u32,
    quality: u8,
    format: image::ImageFormat,
    transparent: bool,
) -> Option<Encoded> {
    let mut out = Vec::new();
    let extension = if format == image::ImageFormat::Png {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        let rgba = resize_transparent(img, width, height);
        PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive)
            .write_image(rgba.as_bytes(), width, height, rgba.color().into())
            .ok()?;
        "png"
    } else if format == image::ImageFormat::Jpeg {
        let mut rgb = img.to_rgb8();
        if rgb.dimensions() != (width, height) {
            rgb =
                image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Triangle);
        }
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .ok()?;
        "jpg"
    } else {
        let processed = if transparent {
            resize_transparent(img, width, height)
        } else {
            let rgb = img.to_rgb8();
            DynamicImage::ImageRgb8(if rgb.dimensions() == (width, height) {
                rgb
            } else {
                image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Triangle)
            })
        };
        if format == image::ImageFormat::WebP {
            let rgba = processed.to_rgba8();
            image::codecs::webp::WebPEncoder::new_lossless(&mut out)
                .write_image(
                    rgba.as_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                )
                .ok()?;
            "webp"
        } else {
            processed
                .write_to(&mut std::io::Cursor::new(&mut out), format)
                .ok()?;
            match format {
                image::ImageFormat::Bmp => "bmp",
                image::ImageFormat::Tiff => "tiff",
                image::ImageFormat::Ico => "ico",
                image::ImageFormat::Gif => "gif",
                _ => return None,
            }
        }
    };
    Some(Encoded {
        bytes: out,
        extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn source_bytes(img: &DynamicImage, format: image::ImageFormat) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn removed_webp_setting_migrates_to_jpg_without_changing_source_id() {
        assert_eq!(Compression::parse("webp"), Some(Compression::Jpg));
        assert_eq!(Compression::from_id(2), Compression::Jpg);
        assert_eq!(Compression::from_id(3), Compression::Source);
        assert_eq!(
            Compression::parse("Same as Source"),
            Some(Compression::Source)
        );
    }

    #[test]
    fn opaque_conversion_checkbox_controls_png_and_webp_without_overriding_output_choice() {
        let mut seed = 0x19283746u32;
        let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(128, 128, |_, _| {
            let mut pixel = [0, 0, 0, 255];
            for channel in &mut pixel[..3] {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *channel = seed as u8;
            }
            image::Rgba(pixel)
        }));
        for source in [image::ImageFormat::Png, image::ImageFormat::WebP] {
            let input = source_bytes(&img, source);
            let decoded = image::load_from_memory(&input).unwrap();
            for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
                for automatic_webp in [false, true] {
                    for opaque_jpg in [false, true] {
                        let expected = if compression == Compression::Jpg && opaque_jpg {
                            image::ImageFormat::Jpeg
                        } else if compression == Compression::Png
                            || (source == image::ImageFormat::WebP && automatic_webp)
                        {
                            image::ImageFormat::Png
                        } else {
                            source
                        };
                        let candidate = encoded_as(
                            &decoded,
                            usize::MAX,
                            95,
                            100,
                            compression,
                            source,
                            automatic_webp,
                            opaque_jpg,
                        )
                        .unwrap();
                        assert_eq!(image::guess_format(&candidate.bytes).unwrap(), expected);
                        let selected = encoded_as(
                            &decoded,
                            input.len(),
                            95,
                            100,
                            compression,
                            source,
                            automatic_webp,
                            opaque_jpg,
                        );
                        if expected == image::ImageFormat::Jpeg {
                            let selected = selected.unwrap();
                            assert_eq!(selected.extension, "jpg");
                            assert!(selected.bytes.len() < input.len());
                        } else if source == image::ImageFormat::WebP && automatic_webp {
                            assert_eq!(selected.unwrap().extension, "png");
                        } else {
                            assert_eq!(selected.is_some(), candidate.bytes.len() < input.len());
                            if let Some(selected) = selected {
                                assert_eq!(image::guess_format(&selected.bytes).unwrap(), expected);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn either_opaque_checkbox_state_preserves_transparency() {
        let mut rgba = RgbaImage::from_pixel(7, 5, image::Rgba([30, 120, 220, 255]));
        rgba.put_pixel(3, 2, image::Rgba([30, 120, 220, 254]));
        let img = DynamicImage::ImageRgba8(rgba);
        for source in [image::ImageFormat::Png, image::ImageFormat::WebP] {
            let input = source_bytes(&img, source);
            let decoded = image::load_from_memory(&input).unwrap();
            for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
                for automatic_webp in [false, true] {
                    for opaque_jpg in [false, true] {
                        let out = encoded_as(
                            &decoded,
                            usize::MAX,
                            95,
                            100,
                            compression,
                            source,
                            automatic_webp,
                            opaque_jpg,
                        )
                        .unwrap();
                        assert_ne!(out.extension, "jpg");
                        assert_eq!(
                            image::load_from_memory(&out.bytes).unwrap().to_rgba8(),
                            decoded.to_rgba8(),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn opaque_checkbox_does_not_disable_jpeg_compression_for_bitmap_or_jpeg_sources() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            48,
            image::Rgb([30, 120, 220]),
        ));
        for source in [image::ImageFormat::Bmp, image::ImageFormat::Jpeg] {
            for opaque_jpg in [false, true] {
                let out = encoded_as(
                    &img,
                    usize::MAX,
                    95,
                    100,
                    Compression::Jpg,
                    source,
                    true,
                    opaque_jpg,
                )
                .unwrap();
                assert_eq!(out.extension, "jpg");
            }
        }
    }

    #[test]
    fn opaque_png_and_webp_become_smaller_jpegs_even_with_an_alpha_channel() {
        let mut seed = 0x19283746u32;
        let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(128, 128, |_, _| {
            let mut pixel = [0, 0, 0, 255];
            for channel in &mut pixel[..3] {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *channel = seed as u8;
            }
            image::Rgba(pixel)
        }));
        for format in [image::ImageFormat::Png, image::ImageFormat::WebP] {
            let input = source_bytes(&img, format);
            let decoded =
                DynamicImage::ImageRgba8(image::load_from_memory(&input).unwrap().to_rgba8());
            assert!(decoded.color().has_alpha());
            for automatic in [false, true] {
                let output = encoded_as(
                    &decoded,
                    input.len(),
                    95,
                    100,
                    Compression::Jpg,
                    format,
                    automatic,
                    true,
                )
                .unwrap();
                assert_eq!(output.extension, "jpg");
                assert_eq!(
                    image::guess_format(&output.bytes).unwrap(),
                    image::ImageFormat::Jpeg
                );
                assert!(output.bytes.len() < input.len());
            }
        }
    }

    #[test]
    fn automatic_conversion_keeps_alpha_and_works_when_png_is_larger() {
        // A one-pixel WebP is smaller than its PNG counterpart. This exercises
        // the compatibility exception using real encoded source sizes.
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([30, 120, 220, 127]),
        ));
        let input = source_bytes(&img, image::ImageFormat::WebP);
        let decoded = image::load_from_memory(&input).unwrap();
        for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
            let output = encoded_as(
                &decoded,
                input.len(),
                95,
                100,
                compression,
                image::ImageFormat::WebP,
                true,
                true,
            )
            .unwrap();
            assert_eq!(output.extension, "png");
            assert!(output.bytes.len() >= input.len());
            assert_eq!(
                image::load_from_memory(&output.bytes).unwrap().to_rgba8(),
                decoded.to_rgba8()
            );
            assert!(encoded_as(
                &decoded,
                input.len(),
                95,
                100,
                compression,
                image::ImageFormat::WebP,
                false,
                true,
            )
            .is_none());
        }
    }

    #[test]
    fn opaque_webp_uses_png_when_selected_jpeg_does_not_shrink() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([30, 120, 220, 255]),
        ));
        let input = source_bytes(&img, image::ImageFormat::WebP);
        let decoded = image::load_from_memory(&input).unwrap();
        assert!(encoded_as(
            &decoded,
            input.len(),
            95,
            100,
            Compression::Jpg,
            image::ImageFormat::WebP,
            false,
            true,
        )
        .is_none());
        let output = encoded_as(
            &decoded,
            input.len(),
            95,
            100,
            Compression::Jpg,
            image::ImageFormat::WebP,
            true,
            true,
        )
        .unwrap();
        assert_eq!(output.extension, "png");
        assert_eq!(
            image::load_from_memory(&output.bytes).unwrap().to_rgba8(),
            decoded.to_rgba8()
        );
    }

    #[test]
    fn one_faint_transparent_pixel_in_png_or_webp_never_becomes_jpeg() {
        let mut rgba = RgbaImage::from_pixel(64, 64, image::Rgba([30, 120, 220, 255]));
        rgba.put_pixel(12, 34, image::Rgba([30, 120, 220, 254]));
        let img = DynamicImage::ImageRgba8(rgba);
        for format in [image::ImageFormat::Png, image::ImageFormat::WebP] {
            let input = source_bytes(&img, format);
            let decoded = image::load_from_memory(&input).unwrap();
            for percent in [25, 50, 75, 100] {
                let output = encoded_as(
                    &decoded,
                    usize::MAX,
                    95,
                    percent,
                    Compression::Jpg,
                    format,
                    true,
                    true,
                )
                .unwrap();
                assert_eq!(output.extension, "png");
                if percent == 100 {
                    assert_eq!(
                        image::load_from_memory(&output.bytes).unwrap().to_rgba8(),
                        decoded.to_rgba8()
                    );
                }
            }
        }
    }

    #[test]
    fn automatic_webp_conversion_does_not_bypass_size_checks_for_png_sources() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([30, 120, 220, 127]),
        ));
        for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
            assert!(encoded_as(
                &img,
                1,
                95,
                100,
                compression,
                image::ImageFormat::Png,
                true,
                true,
            )
            .is_none());
        }
    }

    #[test]
    fn compression_choices_produce_the_requested_format_and_only_replace_smaller_images() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            48,
            image::Rgb([30, 120, 220]),
        ));
        for (compression, source, expected) in [
            (
                Compression::Png,
                image::ImageFormat::Jpeg,
                image::ImageFormat::Png,
            ),
            (
                Compression::Jpg,
                image::ImageFormat::Png,
                image::ImageFormat::Jpeg,
            ),
            (
                Compression::Source,
                image::ImageFormat::Png,
                image::ImageFormat::Png,
            ),
            (
                Compression::Source,
                image::ImageFormat::Jpeg,
                image::ImageFormat::Jpeg,
            ),
            (
                Compression::Source,
                image::ImageFormat::WebP,
                image::ImageFormat::WebP,
            ),
            (
                Compression::Source,
                image::ImageFormat::Bmp,
                image::ImageFormat::Bmp,
            ),
        ] {
            let out =
                encoded_as(&img, usize::MAX, 95, 50, compression, source, false, true).unwrap();
            assert_eq!(image::guess_format(&out.bytes).unwrap(), expected);
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (32, 24));
            assert!(encoded_as(
                &img,
                out.bytes.len(),
                95,
                50,
                compression,
                source,
                false,
                true
            )
            .is_none());
        }
    }
    #[test]
    fn all_compression_choices_keep_transparency() {
        let rgba = RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([40, 180, 210, ((x + y) * 4) as u8])
        });
        let img = DynamicImage::ImageRgba8(rgba.clone());
        for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
            let out = encoded_as(
                &img,
                usize::MAX,
                95,
                100,
                compression,
                image::ImageFormat::Png,
                false,
                true,
            )
            .unwrap();
            let decoded = image::load_from_memory(&out.bytes).unwrap().to_rgba8();
            assert_eq!(decoded, rgba);
        }
    }

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
