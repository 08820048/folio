use folio::buffer;
use gpui::{RenderImage, SvgRenderer};
use image::{DynamicImage, Frame, ImageDecoder, ImageReader, Limits};
use std::{
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

pub enum Content {
    Text(String),
    Image(Arc<RenderImage>),
    /// Too large to hand to the editor, or not UTF-8. The app reads it one
    /// page at a time; this marker only says which viewer to use.
    Large {
        size: u64,
        binary: bool,
    },
}

/// A page of a large text file. Deliberately smaller than the 32 MiB editor
/// limit: the renderer gets one page at a time, never the whole file.
pub const LARGE_TEXT_PAGE: u64 = 64 * 1024;
/// A page of hex is denser than text, so its page is smaller.
pub const LARGE_BINARY_PAGE: u64 = 16 * 1024;

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
        let meta = fs::metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::other("Cannot preview: not a regular file"));
        }
        if meta.len() > buffer::FILE_LIMIT {
            return Ok(Content::Large {
                size: meta.len(),
                binary: probe_binary(path)?,
            });
        }
        return match buffer::read(path) {
            Ok(text) => Ok(Content::Text(text)),
            // A binary or non-UTF-8 file is still worth looking at as hex.
            Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(Content::Large {
                size: meta.len(),
                binary: true,
            }),
            Err(error) => Err(error),
        };
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
        return Err(io::Error::other("Cannot preview: too many pixels"));
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

fn probe_binary(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut bytes = vec![0; 64 * 1024];
    let read = file.read(&mut bytes)?;
    bytes.truncate(read);
    if bytes.contains(&0) {
        return Ok(true);
    }
    Ok(match std::str::from_utf8(&bytes) {
        Ok(_) => false,
        // An invalid sequence that ends the probe may just be a multi-byte
        // character cut in half; an error earlier than that means binary.
        Err(error) => error.error_len().is_some() || error.valid_up_to() + 3 < bytes.len(),
    })
}

pub fn page_size(binary: bool) -> u64 {
    if binary {
        LARGE_BINARY_PAGE
    } else {
        LARGE_TEXT_PAGE
    }
}

pub fn page_count(size: u64, binary: bool) -> u64 {
    size.div_ceil(page_size(binary)).max(1)
}

/// Read one page. Text is decoded lossily (a page boundary may cut a character
/// only in the rarest case); binary is rendered as an offset/hex/ASCII dump.
pub fn read_page(path: &Path, page: usize, binary: bool) -> io::Result<String> {
    let size = page_size(binary);
    let base = page as u64 * size;
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(base))?;
    let mut bytes = vec![0; size as usize];
    let read = file.read(&mut bytes)?;
    bytes.truncate(read);
    if binary {
        Ok(hex_dump(base, &bytes))
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub fn hex_dump(base: u64, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() / 16 * 78 + 16);
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let offset = base + (row * 16) as u64;
        out.push_str(&format!("{offset:08x}  "));
        for column in 0..16 {
            match chunk.get(column) {
                Some(byte) => out.push_str(&format!("{byte:02x} ")),
                None => out.push_str("   "),
            }
            if column == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for byte in chunk {
            out.push(if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    out
}

/// A CommonMark fenced block that cannot be closed by the page text itself.
pub fn text_fence(text: &str) -> String {
    let longest = text
        .lines()
        .map(|line| {
            line.chars()
                .take_while(|character| *character == '~')
                .count()
        })
        .max()
        .unwrap_or(0);
    let fence = "~".repeat((longest + 1).max(4));
    format!("{fence}\n{text}\n{fence}")
}

pub fn size_label(size: u64) -> String {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;
    const GB: f64 = MB * 1024.;
    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{} B", size as u64)
    }
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

        // A small binary file is a hex page rather than a refusal.
        let binary = temp.path().join("blob.bin");
        std::fs::write(&binary, [0_u8, 1, 2, 255]).unwrap();
        assert!(matches!(
            read(&binary).unwrap(),
            Content::Large {
                size: 4,
                binary: true
            }
        ));
    }

    #[test]
    fn large_file_pages_and_hex_are_bounded() {
        assert_eq!(page_count(0, false), 1);
        assert_eq!(page_count(LARGE_TEXT_PAGE, false), 1);
        assert_eq!(page_count(LARGE_TEXT_PAGE + 1, false), 2);
        assert_eq!(page_count(LARGE_BINARY_PAGE + 1, true), 2);

        let dump = hex_dump(0x100, &[0x41, 0x00, 0xff]);
        assert!(dump.contains("00000100  41 00 ff"), "{dump}");
        assert!(dump.contains("|A..|"), "{dump}");

        let fenced = text_fence("one\n~~~\ntwo\n");
        assert!(fenced.starts_with("~~~~"), "{fenced}");
        assert!(fenced.ends_with("~~~~"), "{fenced}");

        let large = "line\n".repeat(LARGE_TEXT_PAGE as usize);
        let path = tempfile::tempdir().unwrap();
        let file = path.path().join("large.txt");
        std::fs::write(&file, large).unwrap();
        assert_eq!(
            read_page(&file, 0, false).unwrap().len(),
            LARGE_TEXT_PAGE as usize
        );
        assert!(!read_page(&file, 1, false).unwrap().is_empty());
    }
}
