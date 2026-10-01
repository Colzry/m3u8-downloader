//! 自定义协议 `m3u8dl://` 深链支持
//!
//! 协议格式（url 与 name 均需 URL 编码）：
//!
//! ```text
//! m3u8dl://add?url=<m3u8下载地址>&name=<视频名称>
//! ```
//!
//! - `url`  : 必填，m3u8 下载地址（需以 `http://` 或 `https://` 开头）
//! - `name` : 选填，视频名称；缺省时尝试从 url 的文件名推导
//!
//! 兼容别名：`url`/`u`/`src`，`name`/`n`/`title`。

use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::WebviewWindow;

/// 一条由深链解析出的下载任务
#[derive(Debug, Clone, Serialize)]
pub struct DeepLinkPayload {
    pub url: String,
    pub name: String,
}

/// 待处理的深链队列。
///
/// 深链可能在窗口/前端尚未就绪时到达（例如冷启动），因此这里先入队，
/// 由前端挂载后通过 `drain_pending_deep_links` 主动拉取，避免事件丢失。
#[derive(Default)]
pub struct DeepLinkQueue(Mutex<Vec<DeepLinkPayload>>);

impl DeepLinkQueue {
    /// 追加若干条待处理任务
    pub fn push_all(&self, mut items: Vec<DeepLinkPayload>) {
        if items.is_empty() {
            return;
        }
        if let Ok(mut queue) = self.0.lock() {
            queue.append(&mut items);
        }
    }

    /// 取出并清空所有待处理任务
    pub fn drain(&self) -> Vec<DeepLinkPayload> {
        self.0
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

/// 解析一条 `m3u8dl://` 链接，非法或缺少下载地址时返回 `None`
pub fn parse(raw: &str) -> Option<DeepLinkPayload> {
    let parsed = tauri::Url::parse(raw).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("m3u8dl") {
        return None;
    }

    let mut target: Option<String> = None;
    let mut name: Option<String> = None;

    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "url" | "u" | "src" => {
                if target.is_none() {
                    target = Some(value.trim().to_string());
                }
            }
            "name" | "n" | "title" => {
                if name.is_none() {
                    name = Some(value.trim().to_string());
                }
            }
            _ => {}
        }
    }

    // 兼容未使用 query 的写法：
    //   a) m3u8dl://https://example.com/a.m3u8   （裸地址直接拼接）
    //   b) m3u8dl://example.com/a.m3u8
    if target.is_none() {
        let host = parsed.host_str().unwrap_or("");
        let path = parsed.path().trim_start_matches('/');

        if host.eq_ignore_ascii_case("http") || host.eq_ignore_ascii_case("https") {
            let mut candidate = format!("{}://{}", host, path);
            if let Some(query) = parsed.query() {
                candidate.push('?');
                candidate.push_str(query);
            }
            target = Some(candidate);
        } else {
            let candidate = match (host.is_empty(), path.is_empty()) {
                (false, false) => format!("{}/{}", host, path),
                (false, true) => host.to_string(),
                (true, false) => path.to_string(),
                (true, true) => String::new(),
            };
            if candidate.starts_with("http://") || candidate.starts_with("https://") {
                target = Some(candidate);
            }
        }
    }

    let target = match target {
        Some(t) if !t.is_empty() => t,
        _ => {
            log::warn!("深链缺少有效的 url 参数，已忽略: {}", raw);
            return None;
        }
    };

    if !(target.starts_with("http://") || target.starts_with("https://")) {
        log::warn!("深链 url 参数不是有效的 http(s) 地址，已忽略: {}", target);
        return None;
    }

    let name = match name {
        Some(n) if !n.is_empty() => n,
        _ => derive_name(&target),
    };

    Some(DeepLinkPayload { url: target, name })
}

/// 从 m3u8 地址推导默认视频名称
fn derive_name(target: &str) -> String {
    if let Ok(u) = tauri::Url::parse(target) {
        if let Some(segment) = u
            .path_segments()
            .and_then(|segments| segments.filter(|s| !s.is_empty()).last())
        {
            let stem = segment.trim_end_matches(".m3u8").trim_end_matches(".M3U8");
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }
    format!("视频_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"))
}

/// 将窗口从后台/最小化状态恢复到前台并短暂置顶
pub fn bring_to_front(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();

    // 短暂置顶，确保窗口压过其它窗口（部分系统会限制后台进程抢焦点）
    if window.set_always_on_top(true).is_ok() {
        let w = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let _ = w.set_always_on_top(false);
        });
    }
}

/// 前端拉取并清空待处理的深链任务
#[tauri::command]
pub fn drain_pending_deep_links(state: tauri::State<'_, DeepLinkQueue>) -> Vec<DeepLinkPayload> {
    state.drain()
}
