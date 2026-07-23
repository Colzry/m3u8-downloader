use chrono::Local;
use fern::Dispatch;
use log::LevelFilter;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub mod rotate;

/// 获取应用程序安装路径（或当前运行目录作为回退方案）
pub fn get_install_dir() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;

    let install_dir = exe_path.parent().ok_or("找不到父目录")?.to_path_buf();

    Ok(install_dir)
}

/// 从安装目录查找是否含有标记日志级别的 flag 文件（无视大小写、后缀）
fn detect_log_level_from_files(install_dir: &PathBuf) -> LevelFilter {
    // 按照优先级排列的关键字列表，越靠前越高优先级
    const LEVEL_MAP: &[(&str, LevelFilter)] = &[
        ("off", LevelFilter::Off),
        ("error", LevelFilter::Error),
        ("warn", LevelFilter::Warn),
        ("info", LevelFilter::Info),
        ("debug", LevelFilter::Debug),
        ("trace", LevelFilter::Trace),
    ];
    // 收集当前目录下所有文件名（不包括子目录）
    let entries = match fs::read_dir(install_dir) {
        Ok(entries) => entries,
        Err(_) => return LevelFilter::Info,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = match path.file_stem().and_then(OsStr::to_str) {
            Some(name) => name.to_lowercase(),
            None => continue,
        };
        for (keyword, level) in LEVEL_MAP {
            if filename == *keyword {
                return *level;
            }
        }
    }
    LevelFilter::Info // 默认级别
}

/// 从 settings.dat 文件中读取日志级别
fn detect_log_level_from_settings(app_handle: &AppHandle) -> Option<LevelFilter> {
    // 尝试加载 settings.dat
    let store_result = app_handle.store("settings.dat");
    let store = match store_result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("无法加载 settings.dat: {}", e);
            return None;
        }
    };

    // 确保数据已从磁盘加载
    if let Err(e) = store.reload() {
        eprintln!("无法从磁盘加载 settings.dat: {}", e);
        return None;
    }

    // 从 Store 中获取 'logLevel' 键的值
    store
        .get("logLevel")
        .and_then(|v| v.as_str().map(|s| s.to_owned()))
        .and_then(|s| s.parse::<LevelFilter>().ok())
}

/// 初始化带有日志滚动的 logging 系统
pub fn setup_logging(app_handle: &AppHandle) -> Result<(), String> {
    // 使用 AppHandle 获取日志目录
    let log_dir = rotate::get_log_dir_path(app_handle)?;

    // 创建 logs 文件夹（如果不存在）
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    }

    // 初始化一次后，清除历史旧日志
    rotate::clean_old_logs(&log_dir);

    // 根据当前日期构建日志文件名（每天一个）
    let today = rotate::get_today_log_file_name().replace(".log", "");

    // 从 settings.dat 检测级别 (优先级最高)
    let level_from_settings = detect_log_level_from_settings(app_handle);

    // 从安装目录检测级别 (优先级第二)
    let level_from_files = get_install_dir()
        .map(|dir| detect_log_level_from_files(&dir))
        .unwrap_or(LevelFilter::Info); // 如果 get_install_dir 失败，默认为 Info

    // 最终日志级别：设置 > 文件 > 默认 Info
    // 如果 level_from_settings 为 None，则回退到 level_from_files。
    let level = level_from_settings.unwrap_or(level_from_files);

    Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(level)
        .chain(std::io::stdout())
        .chain(Box::new(
            rotate::DailyRotatingWriter::new(log_dir.clone(), &today).map_err(|e| e.to_string())?,
        ) as Box<dyn std::io::Write + Send>)
        .apply()
        .map_err(|e| e.to_string())?;

    log::info!("日志模块加载成功");
    log::info!("当前日志级别为: {:?}", level);

    Ok(())
}
