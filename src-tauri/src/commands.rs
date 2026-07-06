use crate::download::{download_m3u8, DownloadOptions};
use crate::download_manager::{DownloadManager, DownloadTask};
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::time::Duration;
use sysinfo::{System, SystemExt};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_store::StoreExt;
use tauri_plugin_updater::UpdaterExt;

#[tauri::command]
pub async fn start_download(
    id: String,
    url: String,
    name: String,
    output_dir: String,
    thread_count: usize,
    app_handle: AppHandle,
    manager: tauri::State<'_, DownloadManager>, // 注入全局管理器
    headers: Option<std::collections::HashMap<String, String>>, // 自定义请求头
) -> Result<(), String> {
    if manager.task_exists(&id).await {
        log::warn!("任务 [{}] 已在运行中，忽略本次请求", id);
        return Ok(());
    }

    let temp_dir = format!("{}/temp_{}", output_dir, id);

    log::info!("Name: [{}], URL: [{}], ID: [{}] - 开始下载", name, url, id);

    // 检查临时目录是否存在，不存在则创建
    let temp_dir_exists = tokio::fs::try_exists(&temp_dir).await.unwrap_or(false);
    if !temp_dir_exists {
        fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
        app_handle
            .emit(
                "create_temp_directory",
                serde_json::json!({
                                "id": id,
                                "isCreatedTempDir": true,
                                "message": "已创建临时下载目录",
                }),
            )
            .ok();
        log::info!("任务 [{}] 已创建临时目录: {}", id, &temp_dir);
    } else {
        log::info!("任务 [{}] 临时目录已存在，继续下载: {}", id, &temp_dir);
    }

    // 创建任务并添加到管理器
    let task = DownloadTask::new(temp_dir.clone());
    let cancelled = task.get_cancel_flag();

    manager
        .add_task(id.clone(), task)
        .await
        .map_err(|e| e.to_string())?;

    // 创建下载选项
    let mut options = DownloadOptions::new();
    if let Some(headers_map) = headers {
        options.headers = headers_map;
    }

    // 开始下载 TS 文件到临时目录
    let download_result = download_m3u8(
        id.clone(),
        &url,
        &name,
        &temp_dir,
        &output_dir,
        thread_count,
        cancelled.clone(),
        app_handle.clone(),
        options,
    )
    .await;

    // 下载完成后，从管理器中移除任务
    if let Err(e) = &download_result {
        log::error!("{} 下载失败: {}", id, e);
        // 下载失败，从管理器移除任务（保留临时目录用于断点续传）
        manager
            .cancel_task(&id)
            .await
            .map_err(|e| format!("取消任务失败: {}", e))?;
        return Err(e.to_string());
    }

    // 检查是否是因为取消而结束的
    // 如果是取消，任务已经从管理器中移除了，不需要再次删除
    // 如果是正常完成，需要删除临时目录
    if !cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        // 下载正常完成（未取消），删除任务并清理临时目录
        manager
            .delete_task(&id)
            .await
            .map_err(|e| format!("删除临时目录失败: {}", e))?;
    }

    // 根据取消标志输出不同的日志
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        log::info!("任务 [{}] 已取消下载", id);
    } else {
        log::info!("任务 [{}] 已下载完成", id);
    }

    Ok(())
}

/// 取消下载任务
///
/// 1. 取消正在运行的下载任务
/// 2. 从管理器中移除任务
/// 3. 保留临时目录以支持断点续传
#[tauri::command]
pub async fn cancel_download(
    id: String,
    manager: tauri::State<'_, DownloadManager>,
) -> Result<(), String> {
    log::info!("取消下载任务: {} (保留临时目录)", id);
    manager.cancel_task(&id).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 删除下载任务并清理临时目录
///
/// 1. 取消正在运行的任务（如果存在）
/// 2. 从管理器中移除任务
/// 3. 删除临时目录和所有下载进度
#[tauri::command]
pub async fn delete_download(
    id: String,
    output_dir: String,
    manager: tauri::State<'_, DownloadManager>,
) -> Result<(), String> {
    log::info!("删除下载任务: {}", id);

    // 1. 先检查任务是否在管理器中
    let task_exists = manager.task_exists(&id).await;

    if task_exists {
        // 任务正在运行，调用 delete_task（会取消并删除临时目录）
        manager
            .delete_task(&id)
            .await
            .map_err(|e| format!("删除任务失败: {}", e))?;
    } else {
        // 任务不在管理器中（已完成或未开始），直接删除临时目录
        log::info!("任务不在管理器中，直接删除临时目录");
        let temp_dir = format!("{}/temp_{}", output_dir, id);

        if tokio::fs::try_exists(&temp_dir).await.unwrap_or(false) {
            tokio::fs::remove_dir_all(&temp_dir)
                .await
                .map_err(|e| format!("删除临时目录失败 ({}): {}", temp_dir, e))?;
            log::info!("已删除临时目录: {}", temp_dir);
        }
    }

    Ok(())
}

/// 获取物理核心数和逻辑线程数
#[tauri::command]
pub fn get_cpu_info() -> (usize, usize) {
    let mut sys = System::new();
    sys.refresh_cpu(); // 刷新CPU信息
    let physical_cores = sys.physical_core_count().unwrap_or(0);
    let logical_cores = sys.cpus().len();
    (physical_cores, logical_cores)
}

/// 删除指定文件
#[tauri::command]
pub async fn delete_file(file_path: String) -> Result<(), String> {
    tokio::fs::remove_file(file_path.clone())
        .await
        .map_err(|e| format!("删除{}文件失败: {}", file_path, e))?;
    Ok(())
}

/// 将设置项保存到 settings.dat
#[tauri::command]
pub async fn save_settings(settings_object: Value, app_handle: AppHandle) -> Result<(), String> {
    let store = app_handle
        .store("settings.dat")
        .map_err(|e| format!("加载Store失败: {}", e))?;

    // 确保传入的是一个 JSON 对象
    let settings_map = settings_object
        .as_object()
        .ok_or("传入的设置不是有效的JSON对象")?;

    // 批量更新 Store 的设置
    for (key, value) in settings_map {
        store.set(key, value.clone());
    }

    // 一次性保存所有更改
    store
        .save()
        .map_err(|e| format!("保存Store配置失败: {}", e))?;

    log::debug!(
        "设置已保存到 settings.dat 中(共{} 个键)",
        settings_map.len()
    );

    Ok(())
}

/// 将设置保存到指定的 store 文件
#[tauri::command]
pub async fn save_store_file(
    file_name: String,
    settings_object: Value,
    app_handle: AppHandle,
) -> Result<(), String> {
    if file_name.trim().is_empty() {
        return Err("文件名不能为空".into());
    }

    let store = app_handle
        .store(&file_name)
        .map_err(|e| format!("加载Store失败({}): {}", file_name, e))?;

    // 确保输入是 JSON 对象
    let settings_map = settings_object
        .as_object()
        .ok_or("传入的设置不是有效的JSON对象")?;

    // 写入所有 key-value
    for (key, value) in settings_map {
        store.set(key, value.clone());
    }

    store
        .save()
        .map_err(|e| format!("保存 Store({}) 失败: {}", file_name, e))?;

    log::info!(
        "设置已保存到 {} 中(共{} 个键)",
        file_name,
        settings_map.len()
    );

    Ok(())
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;

    // 通知前端开始检查更新
    let _ = app.emit(
        "update_status",
        serde_json::json!({
            "status": "checking",
            "message": "正在检查更新..."
        }),
    );

    if let Some(update) = updater.check().await.map_err(|e| e.to_string())? {
        // 有更新
        let mut downloaded: u64 = 0;

        // 通知前端开始下载
        let _ = app.emit(
            "update_status",
            serde_json::json!({
                "status": "downloading",
                "progress": 0,
                "message": "发现新版本，开始下载..."
            }),
        );

        update
            .download_and_install(
                |chunk_length, content_length| {
                    downloaded += chunk_length as u64;
                    let progress = if let Some(total) = content_length {
                        (downloaded as f64 / total as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                    let _ = app.emit(
                        "update_status",
                        serde_json::json!({
                            "status": "downloading",
                            "progress": progress.floor() as u32,
                            "message": format!("下载中: {:.2}%", progress)
                        }),
                    );
                },
                || {
                    let _ = app.emit(
                        "update_status",
                        serde_json::json!({
                            "status": "finished",
                            "progress": 100,
                            "message": "下载完成，正在安装更新..."
                        }),
                    );
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        let _ = app.emit(
            "update_status",
            serde_json::json!({
                "status": "installed",
                "progress": 100,
                "message": "更新安装完成，应用将重启"
            }),
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        app.restart();
    } else {
        // 已是最新版本
        let _ = app.emit(
            "update_status",
            serde_json::json!({
                "status": "latest",
                "progress": 100,
                "message": "已经是最新版本"
            }),
        );
    }

    Ok(())
}

/// 通过 Rust 后端发送系统通知
#[tauri::command]
pub fn send_notification_cmd(app: AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("发送通知失败: {}", e))
}
