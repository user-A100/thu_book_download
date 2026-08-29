use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicUsize},
};

use tokio_util::sync::CancellationToken;

use crate::{convert, download::Downloader, pre_process::Preprocessor};

#[derive(Clone)]
pub struct DownloadOptions {
    pub url: String,
    pub token: String,
    pub thread_number: usize,
    pub quality: u32,
    pub del_img: bool,
    pub auto_resize: bool,
    pub output_root: PathBuf,
}

pub async fn run_download(
    options: DownloadOptions,
    cancel: CancellationToken,
    progress: Option<Arc<AtomicUsize>>,
    total: Option<Arc<AtomicUsize>>,
    stage: Option<Arc<Mutex<String>>>,
) -> Result<PathBuf, String> {
    if let Some(stage) = &stage {
        *stage.lock().unwrap() = "reading".into();
    }
    let pre_processor = Preprocessor::new().map_err(|e| e.to_string())?;
    let task = pre_processor
        .parse(&options.url, &options.token)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(total) = &total {
        total.store(
            task.page_urls.iter().map(Vec::len).sum(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    let save_dir = options.output_root.join(&task.book_real_id);
    if let Some(stage) = &stage {
        *stage.lock().unwrap() = "downloading".into();
    }
    let downloader = Downloader::new().map_err(|e| e.to_string())?;
    let success = downloader
        .download_imgs(task, &save_dir, options.thread_number, cancel, progress)
        .await;
    if !success {
        return Err("下载失败或任务已取消".into());
    }

    let pdf_path = save_dir.with_extension("pdf");
    if let Some(stage) = &stage {
        *stage.lock().unwrap() = "converting".into();
    }
    convert::convert(&save_dir, &pdf_path, options.quality, options.auto_resize)
        .await
        .map_err(|e| e.to_string())?;
    if options.del_img {
        fs::remove_dir_all(&save_dir).map_err(|e| e.to_string())?;
    }
    Ok(pdf_path)
}
