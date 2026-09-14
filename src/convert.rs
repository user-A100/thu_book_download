use image::ImageReader;
use lopdf::content::Content;
use lopdf::{Document, Object, SaveOptions, Stream, dictionary};

use std::collections::HashMap;
use std::{fs, path::Path, sync::Arc};

use tokio::sync::Semaphore;

/// Extract (chapter, page) from file names like "3_12.jpg". Returns None for
/// anything else (e.g. leftover ".tmpXXXX" files from interrupted downloads).
fn page_order(file_name: &str) -> Option<(i32, i32)> {
    let stem = file_name.rsplit_once('.')?.0;
    let (chapter, page) = stem.split_once('_')?;
    Some((chapter.parse().ok()?, page.parse().ok()?))
}

fn get_images(dir: &Path) -> Vec<Arc<Path>> {
    let mut result = Vec::new();
    for entry in
        fs::read_dir(dir).unwrap_or_else(|_| panic!("Can't open directory {}", dir.display()))
    {
        if let Ok(entry) = entry
            && entry.file_type().is_ok_and(|filetype| filetype.is_file())
        {
            let file_name = entry.file_name();
            if let Some(order) = file_name.to_str().and_then(page_order) {
                result.push((order, file_name));
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
        .into_iter()
        .map(|(_, file_name)| Arc::from(dir.join(file_name)))
        .collect()
}

/// Returns the path of the image to embed for each input image. Images that
/// already match the target size are used as-is; only mismatching ones are
/// decoded, resized and written to `intermediate_dir`.
async fn prepare_images(
    imgs: &[Arc<Path>],
    intermediate_dir: &Path,
    quality: u32,
    auto_resize: bool,
) -> Result<Vec<Arc<Path>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut common_size = None;
    if auto_resize {
        let mut size_count: HashMap<(u32, u32), usize> = HashMap::new();
        for img_path in imgs {
            let dims = ImageReader::open(img_path)?.into_dimensions()?;
            *size_count.entry(dims).or_default() += 1;
        }
        common_size = size_count
            .drain()
            .max_by_key(|(_, count)| *count)
            .map(|(size, _)| size);
    }
    if let Some((width, height)) = common_size {
        println!("Auto resizing with width: {width}, height: {height}");
    }

    let mut jobs = Vec::new();
    let mut effective = Vec::with_capacity(imgs.len());
    for img_path in imgs {
        let dims = ImageReader::open(img_path)?.into_dimensions()?;
        let target = match common_size {
            Some((width, height)) => (width / 10 * quality, height / 10 * quality),
            // quality == 10 keeps the original resolution, nothing to scale.
            None if quality >= 10 => dims,
            None => (dims.0 / 10 * quality, dims.1 / 10 * quality),
        };
        if target == dims {
            effective.push(img_path.clone());
        } else {
            let output: Arc<Path> = Arc::from(intermediate_dir.join(img_path.file_name().unwrap()));
            effective.push(output.clone());
            jobs.push((img_path.clone(), output, target));
        }
    }
    if jobs.is_empty() {
        return Ok(effective);
    }

    fs::create_dir_all(intermediate_dir)?;
    let limit = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let semaphore = Arc::new(Semaphore::new(limit));
    let mut handles = Vec::with_capacity(jobs.len());
    for (src, dst, target) in jobs {
        let permit = semaphore.clone().acquire_owned().await?;
        let handle = tokio::task::spawn_blocking(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let _permit = permit;
            if dst.exists() {
                println!("Resize already completed: {}, skip", dst.display());
                return Ok(());
            }
            let img = ImageReader::open(&src)?.decode()?;
            let img = img.resize(
                target.0,
                target.1,
                image::imageops::FilterType::Lanczos3,
            );

            img.save(&dst)?;
            println!("Resize complete: {}", dst.display());
            Ok(())
        });
        handles.push(handle);
    }
    for handle in handles {
        let result = handle
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })
            .and_then(|result| result);
        result?;
    }
    Ok(effective)
}

async fn img2pdf(
    imgs: Vec<Arc<Path>>,
    pdf_path: &Path,
    quality: u32,
    auto_resize: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let intermediate_dir = pdf_path.with_extension("intermediate");
    let imgs = prepare_images(&imgs, &intermediate_dir, quality, auto_resize).await?;

    let mut doc = Document::with_version("2.0");
    let pages_id = doc.new_object_id();
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let total = imgs.len();
    let mut page_objects = Vec::with_capacity(total);
    for (index, img_path) in imgs.iter().enumerate() {
        let image_xobject = lopdf::xobject::image(img_path)?;
        let content = Content { operations: vec![] };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let width = image_xobject.dict.get(b"Width").unwrap().as_i64().unwrap();
        let height = image_xobject.dict.get(b"Height").unwrap().as_i64().unwrap();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
        });
        doc.insert_image(
            page_id,
            image_xobject,
            (0 as f32, 0 as f32),
            (width as f32, height as f32),
        )?;
        page_objects.push(page_id.into());
        println!("Convert complete: {}/{total}", index + 1)
    }

    let count = page_objects.len();
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => page_objects,
        "Count" => count as i32,
    };

    doc.objects.insert(pages_id, Object::Dictionary(pages));

    if intermediate_dir.exists() {
        fs::remove_dir_all(&intermediate_dir)?;
    }
    doc.compress();
    let mut file = std::fs::File::create(pdf_path)?;
    doc.save_with_options(
        &mut file,
        SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .compression_level(9)
            .max_objects_per_stream(count + 2)
            .build(),
    )?;
    //doc.save_modern(&mut file)?;
    Ok(())
}

pub async fn convert(
    dir: &Path,
    pdf_path: &Path,
    quality: u32,
    auto_resize: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let imgs = get_images(dir);
    img2pdf(imgs, pdf_path, quality, auto_resize).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_order_parses_and_filters() {
        assert_eq!(page_order("3_12.jpg"), Some((3, 12)));
        assert_eq!(page_order("0_0.jpg"), Some((0, 0)));
        assert_eq!(page_order(".tmpAbCd1234"), None);
        assert_eq!(page_order("notes.txt"), None);
        assert_eq!(page_order("cover.jpg"), None);
    }

    /// Real-data smoke test for the conversion pipeline. Ignored by default;
    /// run manually with the directory of a downloaded book:
    /// `THUBOOK_TEST_IMG_DIR=<dir> cargo test real_convert -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_convert() {
        let Ok(dir) = std::env::var("THUBOOK_TEST_IMG_DIR") else {
            eprintln!("THUBOOK_TEST_IMG_DIR not set, skipping");
            return;
        };
        let pdf_path = std::env::temp_dir().join("thubookrs-real-convert-test.pdf");
        let _ = std::fs::remove_file(&pdf_path);
        convert(Path::new(&dir), &pdf_path, 10, false)
            .await
            .unwrap();
        let meta = std::fs::metadata(&pdf_path).unwrap();
        assert!(meta.len() > 0);
        println!("PDF size: {} bytes", meta.len());
    }
}
