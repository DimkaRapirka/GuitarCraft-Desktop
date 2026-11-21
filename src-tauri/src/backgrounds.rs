//! User images are decoded into a local library so a moved source file does not break a scene.
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use serde::Serialize;
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct Background {
    id: String,
    path: PathBuf,
}

pub fn list(dir: &Path) -> Vec<Background> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut images: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().into_string().ok()?;
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp")
                .then_some(Background { id, path })
        })
        .collect();
    images.sort_by(|a, b| a.id.cmp(&b.id));
    images
}

pub fn import(dir: &Path, source: &Path) -> Result<Background, String> {
    let metadata = fs::metadata(source).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
        return Err("Выберите изображение размером до 64 МБ".into());
    }
    let mut reader = ImageReader::open(source)
        .and_then(ImageReader::with_guessed_format)
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err("Выберите изображение PNG, JPEG или WebP".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| format!("Не удалось открыть изображение: {e}"))?;
    let image = if image.width() > 2880 || image.height() > 2880 {
        image.resize(2880, 2880, FilterType::Lanczos3)
    } else {
        image
    };

    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = super::files::safe(source.file_stem().and_then(|s| s.to_str()).unwrap_or("Фон"));
    // Limit the UTF-8 byte count as well as the character count for non-Latin file names.
    let stem: String = stem
        .char_indices()
        .take_while(|(index, _)| *index < 140)
        .map(|(_, c)| c)
        .collect();
    let stem = format!("Фон · {stem}");
    let mut pending = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    {
        let mut out = BufWriter::new(pending.as_file_mut());
        image
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    for n in 1.. {
        let id = if n == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem} ({n}).png")
        };
        let path = dir.join(&id);
        match pending.persist_noclobber(&path) {
            Ok(_) => return Ok(Background { id, path }),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                pending = error.file;
            }
            Err(error) => return Err(error.error.to_string()),
        }
    }
    unreachable!("unbounded name counter")
}
