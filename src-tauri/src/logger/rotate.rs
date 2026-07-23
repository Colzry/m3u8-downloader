use chrono::Local;
use std::fs;
use std::io;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_LOG_KEEP_DAYS: i64 = 30; // 只保存最近30天日志

// 获取当日的日志文件名（如 logs/2025-04-05.log）
pub fn get_today_log_file_name() -> String {
    Local::now().format("%Y-%m-%d").to_string() + ".log"
}

// 获取 log 路径，同时创建目录（如果需要）
pub fn get_log_dir_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let log_dir = app_handle
        .path()
        .app_log_dir()
        .map_err(|e| format!("无法获取Tauri应用日志目录: {}", e))?;

    // 在Linux上，是 $XDG_DATA_HOME/{bundleIdentifier}/logs 或 $HOME/.local/share/{bundleIdentifier}/logs 示例：/home/alice/.local/share/com.tauri.dev/logs
    // 在Windows上，是 %{FOLDERID_LocalAppData}/{bundleIdentifier}/logs 示例：C:\Users\Alice\AppData\Local\com.tauri.dev\logs
    // 在macOS上，是 {homeDir}/Library/Logs/{bundleIdentifier} 示例：/Users/Alice/Library/Logs/com.tauri.dev

    Ok(log_dir)
}

// 清除旧日志（大于30天前）
pub fn clean_old_logs(log_dir: &PathBuf) {
    // 判断目录是否存在
    if !log_dir.exists() {
        eprintln!("日志目录不存在，跳过清理");
        return;
    }

    // 获取 ReadDir 迭代器
    let entries = match fs::read_dir(&log_dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("无法读取日志目录 {}: {:?}", log_dir.display(), e);
            return;
        }
    };

    let now = Local::now();

    for entry in entries {
        let path = match entry {
            Ok(e) => e.path(),
            Err(e) => {
                println!("无法读取文件项: {}", e);
                continue;
            }
        };

        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) => {
                println!("无法获取元数据 {}: {}", path.display(), e);
                continue;
            }
        };

        let modified_time = match meta.modified() {
            Ok(modified) => chrono::DateTime::<Local>::from(modified),
            Err(e) => {
                println!("无法获取修改时间 {}: {}", path.display(), e);
                continue;
            }
        };

        if now.signed_duration_since(modified_time).num_days() > MAX_LOG_KEEP_DAYS {
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("删除失败 {}: {}", path.display(), e);
            } else {
                println!("已删除旧日志: {}", path.display());
            }
        }
    }
}

// ============================================================
//  每日日志轮转 Writer：每次写入前检查日期，过天自动切新文件
// ============================================================
pub struct DailyRotatingWriter {
    log_dir: PathBuf,
    current_date: String,
    current_file: std::fs::File,
}

impl DailyRotatingWriter {
    pub fn new(log_dir: PathBuf, today: &str) -> io::Result<Self> {
        let file = Self::open_log_file(&log_dir, today)?;
        Ok(Self {
            log_dir,
            current_date: today.to_string(),
            current_file: file,
        })
    }

    fn open_log_file(log_dir: &PathBuf, date: &str) -> io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join(format!("{}.log", date)))
    }

    fn rotate_if_needed(&mut self) -> io::Result<()> {
        let today = Local::now().format("%Y-%m-%d").to_string();
        if today != self.current_date {
            let new_file = Self::open_log_file(&self.log_dir, &today)?;
            self.current_file = new_file;
            self.current_date = today;
        }
        Ok(())
    }
}

impl io::Write for DailyRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_if_needed()?;
        self.current_file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.current_file.flush()
    }
}
