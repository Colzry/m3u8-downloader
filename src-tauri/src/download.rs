//! M3U8 分片下载模块，支持AES-128加密流媒体解密
//! - 多线程并发下载
//! - 断点续传
//! - 自定请求头

#![allow(deprecated)]
use crate::download_monitor::{run_monitor_task, DownloadMetrics};
use crate::merge::merge_files;
use aes::Aes128;
use anyhow::{anyhow, Result};
use cbc::Decryptor;
use cipher::generic_array::GenericArray;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use rand::{rngs::SmallRng, Rng, SeedableRng};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use std::time::Duration;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, Semaphore},
};

/// 加密信息结构体
/// 用于存储解密TS分片所需的密钥信息
#[derive(Clone, Serialize, Deserialize)]
struct EncryptionInfo {
    key: Vec<u8>,        // AES-128加密密钥（16字节）
    iv: Option<Vec<u8>>, // 初始化向量（16字节），None时使用默认全零IV
}

/// 十六进制字符串转字节向量
/// 示例：hex_to_bytes("0011ff") -> Ok(vec![0x00, 0x11, 0xff])
fn hex_to_bytes(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err(anyhow::anyhow!("Hex string has odd length"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| anyhow::anyhow!("Invalid hex: {}", e))
        })
        .collect()
}

/// 解析M3U8的EXT-X-KEY标签
/// 返回元组：(加密方法, 密钥URI, IV值)
/// 示例输入："METHOD=AES-128,URI="key.php",IV=0X112233..."
fn parse_ext_x_key(line: &str) -> Result<(String, String, Option<String>)> {
    let content = line.trim_start_matches("#EXT-X-KEY:").trim();
    let mut method = String::new();
    let mut uri = String::new();
    let mut iv = None;

    // 分割键值对
    for part in content.split(',') {
        let mut kv = part.splitn(2, '=');
        let key = kv
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid EXT-X-KEY line"))?
            .trim();
        let value = kv
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid EXT-X-KEY line"))?
            .trim()
            .trim_matches('"');
        match key {
            "METHOD" => method = value.to_string(),
            "URI" => uri = value.to_string(),
            "IV" => iv = Some(value.to_string()),
            _ => {}
        }
    }
    Ok((method, uri, iv))
}

/// 将相对URL解析为绝对URL
/// - `base`: 当前M3U8文件的完整URL
/// - `relative`: 可能是绝对URL、以 / 开头的根路径、或相对路径
fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http") {
        relative.to_string()
    } else if relative.starts_with('/') {
        // 绝对路径 - 相对于域名根目录
        let origin = base.split('/').take(3).collect::<Vec<&str>>().join("/");
        format!("{}{}", origin, relative)
    } else {
        // 相对路径 - 相对于M3U8文件所在目录
        let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or(base);
        format!("{}/{}", dir, relative)
    }
}

/// 自定义下载请求头选项
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub headers: HashMap<String, String>,
}

impl DownloadOptions {
    pub fn new() -> Self {
        Self {
            headers: HashMap::new(),
        }
    }
}

pub enum DownloadResult {
    Success(String),   // 成功并且是有效 ts 文件
    Skipped(String),   // 下载成功，但内容无效或空，未写入磁盘
    Cancelled(String), // 因用户取消而中断下载
}

/// 自定义下载请求头
fn preprocess_headers(headers: &HashMap<String, String>) -> reqwest::header::HeaderMap {
    let mut valid_headers = reqwest::header::HeaderMap::new();
    for (key, value) in headers {
        // 尝试添加自定义请求头，如果格式不正确则跳过
        match (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            (Ok(header_name), Ok(header_value)) => {
                valid_headers.insert(header_name, header_value);
            }
            (Err(_), _) => {
                log::warn!("无效的请求头名称，已跳过: {}", key);
            }
            (_, Err(_)) => {
                log::warn!("无效的请求头值，已跳过: {}={}", key, value);
            }
        }
    }
    valid_headers
}

/// 下载单个TS文件（支持加密内容解密）
async fn download_file(
    index: usize, // 传入当前分片的索引，用于计算 IV
    client: &Client,
    url: &str,
    output_path: &str,
    cancelled: &Arc<AtomicBool>,
    encryption: Option<EncryptionInfo>,
    metrics: Arc<DownloadMetrics>,        // metrics参数
    headers: &reqwest::header::HeaderMap, // 预处理后的有效请求头
) -> Result<DownloadResult> {
    // 构建带自定义请求头的请求
    let request = client.get(url).headers(headers.clone());

    let mut response = request.send().await?;
    let mut buffer = Vec::new();

    while let Some(chunk) = response.chunk().await? {
        // 每次下载数据块后立即检查取消
        if cancelled.load(Ordering::Relaxed) {
            // 主动清理已下载的部分文件
            fs::remove_file(output_path).await.ok();
            return Ok(DownloadResult::Cancelled(url.to_string()));
        }

        // 记录下载数据
        let chunk_len = chunk.len();
        buffer.extend_from_slice(&chunk);
        metrics.record_chunk(chunk_len).await; // 替换原有的计数器更新
    }

    // 判断是否为空
    if buffer.is_empty() {
        log::warn!("[{}] 返回空数据，标记为 Skipped", url);
        return Ok(DownloadResult::Skipped(url.to_string()));
    }

    // 检查是否 HTML/XML 内容（基于 Content-Type 头）
    let content_type = response
        .headers()
        .get("Content-Type")
        .and_then(|ct| ct.to_str().ok())
        .unwrap_or("");

    if content_type.starts_with("text/html") || content_type.contains("xml") {
        log::warn!(
            "[{}] Content-Type 为 HTML/XML ({}), 视为网络错误以便重试",
            url,
            content_type
        );
        return Err(anyhow!(
            "服务器返回了 HTML/XML 内容而非 TS 分片，Content-Type: {}",
            content_type
        ));
    }

    // 基于内容的兜底检测：如果 Content-Type 不可信，检查数据头部
    // 有效 TS 分片以 0x47 同步字节开头，HTML 以 '<' (0x3C) 开头
    if buffer.len() >= 4 {
        let first_byte = buffer[0];
        // 检测 HTML 内容（以 '<' 开头，如 <html, <!DOCTYPE, <head 等）
        if first_byte == b'<' {
            let preview = String::from_utf8_lossy(&buffer[..buffer.len().min(200)]);
            log::warn!(
                "[{}] 内容以 '<' 开头，疑似 HTML 响应（预览: {}），视为网络错误以便重试",
                url,
                preview.chars().take(80).collect::<String>()
            );
            return Err(anyhow!("服务器返回了 HTML 内容而非 TS 分片"));
        }
        // 检测 JSON 错误响应（以 '{' 或 '[' 开头）
        if first_byte == b'{' || first_byte == b'[' {
            let preview = String::from_utf8_lossy(&buffer[..buffer.len().min(200)]);
            log::warn!(
                "[{}] 内容以 JSON 开头，疑似错误响应（预览: {}），视为网络错误以便重试",
                url,
                preview.chars().take(80).collect::<String>()
            );
            return Err(anyhow!("服务器返回了 JSON 内容而非 TS 分片"));
        }
    }

    // AES-128解密处理
    let data: Vec<u8> = if let Some(enc) = encryption {
        // HLS标准：如果IV为空，则使用分片的Media Sequence Number（索引）作为IV
        let iv_vec = enc.iv.unwrap_or_else(|| {
            let mut iv = vec![0u8; 16];
            // 将 index (usize) 转为 u64，然后按大端字节序写入 IV 的后 8 个字节
            let index_bytes = (index as u64).to_be_bytes();
            iv[8..16].copy_from_slice(&index_bytes);
            iv
        });

        let key = GenericArray::from_slice(&enc.key);
        let iv = GenericArray::from_slice(&iv_vec);

        let decryptor = Decryptor::<Aes128>::new(key, iv);

        // buffer 之后不再使用，直接 move 进解密器，避免 clone
        let mut buf = buffer;
        let decrypted = decryptor
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .map_err(|e| anyhow!("Decryption failed: {:?}", e))?;

        decrypted.to_vec()
    } else {
        buffer
    };

    // 写入解密后的文件
    let mut file = fs::File::create(output_path).await?;
    file.write_all(&data).await?;
    Ok(DownloadResult::Success(output_path.to_string()))
}

/// 分片信息结构
#[derive(Serialize, Deserialize)]
struct SegmentMetadata {
    url: String,
    local_path: String,
    encryption: Option<EncryptionInfo>,
}

async fn validate_m3u8_response(
    status: StatusCode,
    text: &str,
    content_type: Option<&str>,
) -> Result<()> {
    // 状态码验证
    if !status.is_success() {
        return Err(match status.as_u16() {
            403 => anyhow::anyhow!("403 Forbidden：服务器拒绝访问，可能需要添加请求头"),
            404 => anyhow::anyhow!("404 Not Found：地址无效或文件不存在"),
            code => anyhow::anyhow!("请求失败，状态码：{}", code),
        });
    }

    // Content-Type 验证
    if let Some(ct) = content_type {
        let ct_lower = ct.to_lowercase();
        if !(ct_lower.contains("mpegurl")
            || ct_lower.contains("m3u8")
            || ct_lower.contains("plain")
            || ct_lower.contains("text")
            || ct_lower.contains("application/octet-stream"))
        {
            return Err(anyhow::anyhow!("Content-Type 不匹配 M3U8 文件：{}", ct));
        }
    }

    // 内容验证
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(anyhow::anyhow!("M3U8 内容无效：缺少 #EXTM3U 标识"));
    }

    Ok(())
}

/// M3U8下载主函数
pub async fn download_m3u8(
    id: String,                 // 下载任务唯一标识
    url: &str,                  // M3U8文件URL
    name: &str,                 // 输出文件名
    temp_dir: &str,             // ts文件下载目录
    output_dir: &str,           // MP4视频输出目录
    concurrency: usize,         // 并发线程数
    cancelled: Arc<AtomicBool>, // 取消标志
    app_handle: AppHandle,      // Tauri应用句柄
    options: DownloadOptions,   // 下载选项（包含自定义headers等）
) -> Result<()> {
    // 创建输出目录
    fs::create_dir_all(temp_dir).await?;

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(concurrency)
        .build()
        .map_err(|e| anyhow!("创建 HTTP Client 失败: {}", e))?;
    // 预处理headers，只验证一次
    let headers = preprocess_headers(&options.headers);
    log::info!("headers: {:#?}", headers);

    // --- 步骤 1: 解析M3U8，收集所有分片信息 ---
    // 分片元数据文件路径
    let segments_metadata_path = format!("{}/segments.json", temp_dir);
    // 添加了 usize，用于存储 index
    let mut all_ts_segments: Vec<(usize, String, String, Option<EncryptionInfo>)> = Vec::new();

    // 尝试从保存的元数据文件中加载分片信息
    if tokio::fs::metadata(&segments_metadata_path).await.is_ok() {
        log::info!("从本地加载分片元数据: {}", segments_metadata_path);
        let metadata_content = tokio::fs::read_to_string(&segments_metadata_path).await?;
        let segments_metadata: Vec<SegmentMetadata> = serde_json::from_str(&metadata_content)?;

        // 转换为原始格式，利用 enumerate 恢复 index
        for (index, segment) in segments_metadata.into_iter().enumerate() {
            all_ts_segments.push((index, segment.url, segment.local_path, segment.encryption));
        }
    } else {
        // 第一次下载，需要解析M3U8文件
        let request = client.get(url).headers(headers.clone());
        let raw_response = request.send().await?;
        let status = raw_response.status();
        let content_type = raw_response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let response_text = raw_response.text().await?;

        // 验证 M3U8
        validate_m3u8_response(status, &response_text, content_type.as_deref()).await?;

        // 检测是否为 Master Playlist（包含 #EXT-X-STREAM-INF）
        // 如果是，选择最高码率的子流继续解析
        let (response_text, parse_base_url) = if response_text
            .lines()
            .any(|l| l.starts_with("#EXT-X-STREAM-INF"))
        {
            log::info!("检测到 Master Playlist，正在选择最高码率子流...");
            let mut best_bandwidth = 0u64;
            let mut best_url = String::new();
            let mut pending_bandwidth = 0u64;

            for line in response_text.lines() {
                let line = line.trim();
                if line.starts_with("#EXT-X-STREAM-INF") {
                    pending_bandwidth = line
                        .split(',')
                        .find(|p| p.trim().starts_with("BANDWIDTH="))
                        .and_then(|p| p.trim().strip_prefix("BANDWIDTH="))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                } else if !line.is_empty() && !line.starts_with('#') {
                    if pending_bandwidth > best_bandwidth {
                        best_bandwidth = pending_bandwidth;
                        best_url = line.to_string();
                    }
                    pending_bandwidth = 0;
                }
            }

            if best_url.is_empty() {
                return Err(anyhow!("Master Playlist 中未找到有效的子流"));
            }

            let sub_url = resolve_url(url, &best_url);
            log::info!("已选择子流 (bandwidth={}): {}", best_bandwidth, sub_url);

            let sub_response = client.get(&sub_url).headers(headers.clone()).send().await?;
            let sub_status = sub_response.status();
            let sub_ct = sub_response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let sub_text = sub_response.text().await?;
            validate_m3u8_response(sub_status, &sub_text, sub_ct.as_deref()).await?;
            // 子流 URL 作为后续解析的基准 URL
            (sub_text, sub_url)
        } else {
            (response_text, url.to_string())
        };

        let mut current_encryption = None;
        let mut ts_index = 0;

        for line in response_text.lines() {
            let line = line.trim();
            if line.starts_with("#EXT-X-KEY:") {
                let (method, key_uri, iv_str) = parse_ext_x_key(line)?;
                if method.to_uppercase() == "AES-128" {
                    let key_url = resolve_url(&parse_base_url, &key_uri);

                    let key_response = client
                        .get(&key_url)
                        .headers(headers.clone())
                        .send()
                        .await?
                        .bytes()
                        .await?;
                    let key = key_response.to_vec();

                    let iv = iv_str.as_ref().and_then(|iv_raw| {
                        let hex = iv_raw.strip_prefix("0x").unwrap_or(iv_raw);
                        hex_to_bytes(hex).ok()
                    });

                    current_encryption = Some(EncryptionInfo { key, iv });
                } else {
                    current_encryption = None;
                }
                continue;
            }

            // 收集TS分片任务
            // 支持带查询参数的URL，如 segment.ts?token=abc
            if !line.starts_with('#') && line.contains(".ts") {
                let ts_url = resolve_url(&parse_base_url, line);
                let filename = format!("{}/part_{}.ts", temp_dir, ts_index);
                all_ts_segments.push((ts_index, ts_url, filename, current_encryption.clone()));
                ts_index += 1;
            }
        }

        // 保存分片元数据到文件，供后续断点续传使用
        let segments_metadata: Vec<SegmentMetadata> = all_ts_segments
            .iter()
            .map(|(_, url, local_path, encryption)| SegmentMetadata {
                url: url.clone(),
                local_path: local_path.clone(),
                encryption: encryption.clone(),
            })
            .collect();

        let metadata_json = serde_json::to_string(&segments_metadata)?;
        tokio::fs::write(&segments_metadata_path, metadata_json).await?;
        log::info!("已保存分片元数据到: {}", segments_metadata_path);
    }

    if all_ts_segments.is_empty() {
        log::warn!("M3U8 [{} {}] 中未找到 .ts 分片", id, name);
        return Err(anyhow::anyhow!("M3U8中未找到任何.ts分片"));
    }

    // --- 步骤 2: 断点续传检查 (基于 Manifest 文件) ---
    let total_chunks = all_ts_segments.len();
    let metrics = Arc::new(DownloadMetrics::new(total_chunks));

    // 不再使用 Mutex 争抢收集文件名，直接从 M3U8 解析列表构建出最终顺序
    let final_ts_files: Vec<String> = all_ts_segments
        .iter()
        .map(|(_, _, path, _)| path.clone())
        .collect();

    let manifest_path = format!("{}/progress.dat", temp_dir);

    // --- 步骤 3: 启动速度监控任务（仅启动一次，贯穿所有重试轮次）---
    let speed_handle = run_monitor_task(
        id.clone(),
        Arc::clone(&cancelled),
        Arc::clone(&metrics),
        app_handle.clone(),
    )
    .await;

    // ==========================================
    //  总体下载重试循环
    //  当分片未集齐时，自动重新评估缺失分片并重试下载
    // ==========================================
    const MAX_OVERALL_RETRIES: usize = 5;
    let mut overall_retries = 0;
    let mut is_first_pass = true;

    loop {
        // 存储真正需要下载的任务
        let mut pending_downloads = Vec::new();

        // 加载清单文件，重新评估已完成的分片
        let mut completed_segment_names = HashSet::new();
        if let Ok(file) = tokio::fs::File::open(&manifest_path).await {
            let reader = BufReader::new(file);
            let mut lines = reader.lines();
            while let Some(line) = lines.next_line().await? {
                if !line.trim().is_empty() {
                    completed_segment_names.insert(line);
                }
            }
        }
        log::info!(
            "任务 [{}]: 从清单文件中加载了 {} 条已完成记录 (第 {} 轮)",
            id,
            completed_segment_names.len(),
            overall_retries + 1
        );

        for (index, ts_url, filename, encryption) in &all_ts_segments {
            let relative_name = match Path::new(&filename).file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            if completed_segment_names.contains(&relative_name) {
                match tokio::fs::metadata(&filename).await {
                    Ok(metadata) if metadata.len() > 0 => {
                        // 仅首轮更新进度计数器，避免重试轮次重复计数
                        if is_first_pass {
                            let file_size = metadata.len() as usize;
                            metrics.completed_chunks.fetch_add(1, Ordering::Relaxed);
                            metrics
                                .downloaded_bytes
                                .fetch_add(file_size, Ordering::Relaxed);
                            metrics.update_total_bytes(file_size);
                        }
                    }
                    _ => {
                        // 清单有记录但文件丢失/为空 → 重新下载
                        pending_downloads.push((
                            *index,
                            ts_url.clone(),
                            filename.clone(),
                            encryption.clone(),
                        ));
                    }
                }
            } else {
                // 清单无记录 → 需要下载
                pending_downloads.push((
                    *index,
                    ts_url.clone(),
                    filename.clone(),
                    encryption.clone(),
                ));
            }
        }
        is_first_pass = false;

        log::info!(
            "任务 [{}]: 总分片 {}, 清单已完成 {}, 本轮待下载 {} (第 {} 轮)",
            id,
            total_chunks,
            completed_segment_names.len(),
            pending_downloads.len(),
            overall_retries + 1
        );

        // 如果本轮没有待下载分片，说明全部集齐，退出循环
        if pending_downloads.is_empty() {
            log::info!("任务 [{}] 所有分片均已就绪，准备合并", id);
            break;
        }

        // --- 步骤 4: 启动下载任务 (只下载 pending_downloads) ---
        // 创建一个线程安全的清单文件写入器
        let manifest_writer = Arc::new(Mutex::new(
            tokio::fs::File::options()
                .append(true)
                .create(true)
                .open(&manifest_path)
                .await?,
        ));

        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::new();

        for (index, ts_url, filename, encryption) in pending_downloads {
            let client = client.clone();
            let semaphore = Arc::clone(&semaphore);
            let cancelled = Arc::clone(&cancelled);
            let metrics = Arc::clone(&metrics);
            let manifest_writer = Arc::clone(&manifest_writer);
            let headers = headers.clone();

            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await?;

                const MAX_RETRIES: usize = 15;
                for attempt in 1..=MAX_RETRIES {
                    if cancelled.load(Ordering::Relaxed) {
                        return Ok::<(), anyhow::Error>(());
                    }
                    let result = download_file(
                        index, // 传入索引，用于 IV 降级处理
                        &client,
                        &ts_url,
                        &filename,
                        &cancelled,
                        encryption.clone(),
                        metrics.clone(),
                        &headers,
                    )
                    .await;

                    match result {
                        Ok(DownloadResult::Success(f)) => {
                            log::debug!("分片 [{}] 下载成功（尝试次数 {}）", f, attempt);

                            if let Some(relative_name) =
                                Path::new(&f).file_name().and_then(|s| s.to_str())
                            {
                                let mut writer = manifest_writer.lock().await;
                                writer
                                    .write_all(format!("{}\n", relative_name).as_bytes())
                                    .await?;
                                writer.flush().await?;
                            }

                            // 将已完成计数器 +1
                            metrics.completed_chunks.fetch_add(1, Ordering::Relaxed);
                            return Ok(());
                        }
                        Ok(DownloadResult::Skipped(f)) => {
                            log::warn!(
                                "分片 [{}] 返回空数据（尝试 {}/{}），稍后重试",
                                f,
                                attempt,
                                MAX_RETRIES
                            );
                            if attempt < MAX_RETRIES {
                                // 空响应可能是网络波动，退避后重试
                                let base_delay_secs = (1 << (attempt - 1)).min(10);
                                let mut rng = SmallRng::from_entropy();
                                let random_millis = rng.gen_range(0..1000);
                                let total_delay = Duration::from_secs(base_delay_secs as u64)
                                    + Duration::from_millis(random_millis);
                                log::info!("分片 [{}] 空数据退避，等待 {:?}", f, total_delay);
                                tokio::time::sleep(total_delay).await;
                            } else {
                                log::error!("分片 [{}] 所有重试均返回空数据，放弃该分片", f);
                                // 最终仍然为空，标记为 Skipped 不计入 completed_chunks
                                return Ok(());
                            }
                        }
                        Ok(DownloadResult::Cancelled(f)) => {
                            log::debug!("分片 [{}] 因取消而中断", f);
                            return Ok(());
                        }
                        Err(e) => {
                            log::warn!(
                                "分片 [{}] 第 {} 次下载失败，原因：{}",
                                filename,
                                attempt,
                                e
                            );
                            if attempt < MAX_RETRIES {
                                // 指数退避和随机抖动
                                let base_delay_secs = (1 << (attempt - 1)).min(10);

                                let mut rng = SmallRng::from_entropy();
                                let random_millis = rng.gen_range(0..1000);

                                let total_delay = Duration::from_secs(base_delay_secs as u64)
                                    + Duration::from_millis(random_millis);

                                log::info!("分片 [{}] 正在退避，等待 {:?}", filename, total_delay);
                                tokio::time::sleep(total_delay).await;
                            } else {
                                log::error!(
                                    "分片 [{}] 所有重试失败: {:?}, 将在下一轮重试",
                                    filename,
                                    e
                                );
                                // 不设置 cancelled，交由外层重试循环处理
                            }
                        }
                    }
                }
                // 该分片本轮重试耗尽，返回 Ok 让外层循环决定是否重试
                Ok(())
            }));
        }

        // --- 步骤 5: 等待本轮所有下载任务完成 ---
        // 不传播单个任务错误，由外层重试循环统一处理未完成的分片
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {} // 任务正常完成
                Ok(Err(e)) => log::warn!("分片下载任务异常退出: {}", e),
                Err(e) => log::error!("分片下载任务崩溃: {}", e),
            }
        }

        // 从清单文件统计实际完成数（比依赖内存计数器更可靠）
        let completed_count = {
            let mut count = 0;
            if let Ok(file) = tokio::fs::File::open(&manifest_path).await {
                let reader = BufReader::new(file);
                let mut lines = reader.lines();
                while let Some(line) = lines.next_line().await? {
                    if !line.trim().is_empty() {
                        count += 1;
                    }
                }
            }
            count
        };

        // 用户主动取消 → 退出循环
        if cancelled.load(Ordering::Relaxed) {
            log::info!(
                "任务 [{}] 未完成下载。预期: {}, 已完成: {}. 任务已被取消",
                id,
                total_chunks,
                completed_count
            );
            break;
        }

        // 所有分片已集齐 → 退出循环
        if completed_count >= total_chunks {
            log::info!("任务 [{}] 所有分片均已就绪，准备合并", id);
            break;
        }

        // 未集齐 → 检查是否达到最大重试次数
        overall_retries += 1;
        if overall_retries >= MAX_OVERALL_RETRIES {
            log::error!(
                "任务 [{}] 经过 {} 轮尝试仍未能集齐所有分片。预期: {}, 实际: {}. 下载失败",
                id,
                MAX_OVERALL_RETRIES,
                total_chunks,
                completed_count
            );
            cancelled.store(true, Ordering::SeqCst);
            break;
        }

        log::warn!(
            "任务 [{}] 第 {} 轮未能集齐所有分片 (已集齐 {}/{})，3秒后开始第 {} 轮重试...",
            id,
            overall_retries,
            completed_count,
            total_chunks,
            overall_retries + 1
        );
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    } // end of overall retry loop

    // 等待速度监控任务退出
    speed_handle.await?;

    // 最终检查：如果被取消，返回 Ok（由上层处理）
    if cancelled.load(Ordering::Relaxed) {
        log::warn!("任务 [{}] 检测已被取消，结束下载", id);
        return Ok(());
    }

    // --- 步骤 6: 合并 TS 文件为 MP4 ---
    merge_files(
        id.clone(),
        &name,
        final_ts_files,
        &temp_dir,
        &output_dir,
        app_handle.clone(),
    )
    .await?;

    Ok(())
}
