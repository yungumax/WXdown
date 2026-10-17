//! 下载引擎：探测长度 → 流式落盘，带进度回调、重试、断点续传与暂停。
//!
//! 公众号的媒体（图片/视频/语音）都是单文件直链，量小图多，
//! 用"每文件一条流 + 定期汇报"就够。

use crate::error::{Result, WxError};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    /// 平均速率（字节/秒）
    pub speed_bps: f64,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub retries: usize,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self { retries: 3 }
    }
}

/// 下载一个文件到 `dest`，进度回调带累计字节数。
pub async fn download(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    opts: &DownloadOptions,
    progress: ProgressFn,
) -> Result<u64> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let mut delay = Duration::from_millis(500);
    let mut last_err = WxError::Unavailable("未知错误".into());

    for attempt in 0..=opts.retries {
        if attempt > 0 {
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        match download_once(http, url, dest, &progress).await {
            Ok(size) => {
                progress(Progress { downloaded: size, total: size, speed_bps: 0.0 });
                return Ok(size);
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn download_once(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    progress: &ProgressFn,
) -> Result<u64> {
    let resp = http.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(WxError::Unavailable(format!("下载失败 HTTP {status}")));
    }
    let total = resp.content_length().unwrap_or(0);

    let downloaded = Arc::new(AtomicU64::new(0));
    let reporter = spawn_reporter(downloaded.clone(), total, progress.clone());

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut last_flush = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        if last_flush.elapsed() > Duration::from_millis(300) {
            tokio::io::AsyncWriteExt::flush(&mut file).await?;
            last_flush = Instant::now();
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;

    reporter.abort();
    let size = downloaded.load(Ordering::Relaxed);
    if total > 0 && size != total {
        return Err(WxError::IncompleteDownload { expected: total, actual: size });
    }
    Ok(size)
}

fn spawn_reporter(
    downloaded: Arc<AtomicU64>,
    total: u64,
    progress: ProgressFn,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let now = downloaded.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            progress(Progress {
                downloaded: now,
                total,
                speed_bps: if elapsed > 0.0 { now as f64 / elapsed } else { 0.0 },
            });
        }
    })
}

/// 下载结束形态：完整落盘，或因暂停而中止（进度保留在 .part，等待续传）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEnd {
    Completed(u64),
    Paused(u64),
}

/// 暂停开关：置 true 后下载循环在下一个分块退出（进度保留在 .part 文件里）。
pub type PauseFlag = Arc<std::sync::atomic::AtomicBool>;

/// 可暂停、可断点续传的下载：进度写在 `<目标>.part`，完成后改名为目标。
pub async fn download_resumable(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    opts: &DownloadOptions,
    progress: ProgressFn,
    pause: Option<PauseFlag>,
) -> Result<DownloadEnd> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    let part = part_path(dest);

    let mut delay = Duration::from_millis(500);
    let mut last_err = WxError::Unavailable("未知错误".into());

    for attempt in 0..=opts.retries {
        if attempt > 0 {
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        if pause.as_ref().map(|f| f.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(false) {
            return Ok(DownloadEnd::Paused(part_len(&part)));
        }
        match download_resumable_once(http, url, &part, &progress, pause.clone()).await {
            Ok(DownloadEnd::Completed(size)) => {
                tokio::fs::rename(part, dest).await?;
                progress(Progress { downloaded: size, total: size, speed_bps: 0.0 });
                return Ok(DownloadEnd::Completed(size));
            }
            Ok(DownloadEnd::Paused(size)) => {
                return Ok(DownloadEnd::Paused(size));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn part_path(dest: &Path) -> PathBuf {
    match dest.extension() {
        Some(ext) => dest.with_extension(format!(
            "{}.part",
            ext.to_string_lossy()
        )),
        None => PathBuf::from(format!("{}.part", dest.display())),
    }
}

fn part_len(part: &Path) -> u64 {
    std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)
}

async fn download_resumable_once(
    http: &reqwest::Client,
    url: &str,
    part: &Path,
    progress: &ProgressFn,
    pause: Option<PauseFlag>,
) -> Result<DownloadEnd> {
    use std::sync::atomic::Ordering;
    use tokio::io::AsyncWriteExt;

    let offset = part_len(part);
    let mut req = http.get(url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let resp = req.send().await?;
    let status = resp.status();
    // 416 = 断点越界：.part 可能已经完整。核对总长一致则视为完成，
    // 不一致则丢弃 .part，外层重试时从头下载。
    if status.as_u16() == 416 {
        let total_remote = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next().and_then(|t| t.parse::<u64>().ok()));
        if total_remote == Some(offset) && offset > 0 {
            return Ok(DownloadEnd::Completed(offset));
        }
        let _ = tokio::fs::remove_file(part).await;
        return Err(WxError::Unavailable("断点越界，已重置续传".into()));
    }
    if !status.is_success() {
        return Err(WxError::Unavailable(format!("下载失败 HTTP {status}")));
    }
    // 服务器不支持 Range（整包 200）→ 从头重来
    let restart = status.as_u16() == 200 && offset > 0;
    let base = if restart { 0 } else { offset };
    let total = resp.content_length().map(|l| l + base).unwrap_or(0);

    let downloaded = Arc::new(AtomicU64::new(base));
    let reporter = {
        let downloaded = downloaded.clone();
        let progress = progress.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let now = downloaded.load(Ordering::Relaxed);
                let elapsed = start.elapsed().as_secs_f64();
                progress(Progress {
                    downloaded: now,
                    total,
                    speed_bps: if elapsed > 0.0 { now as f64 / elapsed } else { 0.0 },
                });
            }
        })
    };

    let mut file = if restart {
        tokio::fs::File::create(part).await?
    } else {
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(part)
            .await?
    };

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if pause.as_ref().map(|f| f.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(false) {
            reporter.abort();
            file.flush().await?;
            let size = downloaded.load(Ordering::Relaxed);
            return Ok(DownloadEnd::Paused(size));
        }
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().await?;

    reporter.abort();
    let size = downloaded.load(Ordering::Relaxed);
    if total > 0 && size != total {
        return Err(WxError::IncompleteDownload { expected: total, actual: size });
    }
    Ok(DownloadEnd::Completed(size))
}

#[cfg(test)]
mod tests {
    #[test]
    fn part_path_shape() {
        let dest = std::path::Path::new("a/01.jpg");
        let part = super::part_path(dest);
        assert_eq!(part, std::path::Path::new("a/01.jpg.part"));
    }
}
