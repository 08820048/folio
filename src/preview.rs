use folio::buffer;
use gpui::{RenderImage, SvgRenderer};
use image::{DynamicImage, Frame, ImageDecoder, ImageReader, Limits};
use std::{
    io::{self, Cursor},
    path::Path,
    sync::Arc,
};

pub enum Content {
    Text(String),
    Image(Arc<RenderImage>),
}

pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg"
    )
}

pub fn read(path: &Path) -> io::Result<Content> {
    if !is_image(path) {
        return buffer::read(path).map(Content::Text);
    }
    let bytes = buffer::read_bytes(path)?;
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        // GPUI caps SVG raster dimensions at 8192 pixels; parsing/rasterization stays on the worker.
        return SvgRenderer::new(Arc::new(()))
            .render_single_frame(&bytes, 1.)
            .map(Content::Image)
            .map_err(io::Error::other);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(io::Error::other)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 32 * 1024 * 1024 {
        return Err(io::Error::other("图片像素过多，无法预览"));
    }
    let orientation = decoder.orientation().map_err(io::Error::other)?;
    // ponytail: animated formats show the first frame; add bounded animation decoding if playback is requested.
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(io::Error::other)?;
    decoded.apply_orientation(orientation);
    let mut pixels = decoded.into_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Content::Image(Arc::new(RenderImage::new(vec![
        Frame::new(pixels),
    ]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_images_decode_without_becoming_editable_text() {
        let temp = tempfile::tempdir().unwrap();
        let pixels = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Gif,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
            image::ImageFormat::Tiff,
            image::ImageFormat::Ico,
        ] {
            let path = temp.path().join(format!(
                "image.{}",
                format.extensions_str()[0].to_uppercase()
            ));
            let image = DynamicImage::ImageRgba8(pixels.clone());
            if format == image::ImageFormat::Jpeg {
                image.to_rgb8().save_with_format(&path, format).unwrap();
            } else {
                image.save_with_format(&path, format).unwrap();
            }
            let Content::Image(decoded) = read(&path).unwrap() else {
                panic!("image treated as text")
            };
            assert_eq!(u32::from(decoded.size(0).width), 3);
            assert_eq!(u32::from(decoded.size(0).height), 2);
            assert!(decoded.as_bytes(1).is_none());
            if format == image::ImageFormat::Png {
                assert_eq!(&decoded.as_bytes(0).unwrap()[..4], &[30, 20, 10, 255]);
            }
        }
        let svg = temp.path().join("vector.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#).unwrap();
        assert!(matches!(read(&svg).unwrap(), Content::Image(_)));
        let invalid = temp.path().join("invalid.png");
        std::fs::write(&invalid, "not an image").unwrap();
        assert!(read(&invalid).is_err());
        let text = temp.path().join("main.rs");
        std::fs::write(&text, "// normal text").unwrap();
        assert!(matches!(read(&text).unwrap(), Content::Text(_)));
    }
}
