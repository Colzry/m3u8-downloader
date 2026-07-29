use crate::commands::{
    cancel_download, check_update, delete_download, delete_file, force_merge, get_cpu_info,
    save_settings, save_store_file, send_notification_cmd, start_download,
};
use crate::download_manager::DownloadManager;
use tauri::{
    async_runtime,
    tray::{MouseButton, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_store::StoreExt;

pub mod commands;
mod download;
mod download_manager;
mod download_monitor;
mod logger;
mod merge;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 1. 创建 Tokio 运行时
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    // 设置 Tauri 的异步运行时
    async_runtime::set(runtime.handle().clone());

    // 2. 启动 Tauri 应用程序
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 多次启动聚焦主窗口
            if let Some(window) = get_or_create_main_window(app) {
                if let Ok(true) = window.is_minimized() {
                    let _ = window.unminimize();
                }
                if let Ok(false) = window.is_visible() {
                    let _ = window.show();
                }
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(DownloadManager::new()) // 注册下载全局状态管理
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            if let Err(e) = logger::setup_logging(&app.handle()) {
                log::error!("初始化Tauri日志失败：{}", e);
                eprintln!("初始化Tauri日志失败：{}", e);
            }

            // 初始化托盘
            enable_tray(app)?;

            // ==========================================
            // 为初始主窗口绑定关闭拦截逻辑
            // ==========================================
            if let Some(main_window) = app.get_webview_window("main") {
                let w = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api: _api, .. } = event {
                        let app_handle = w.app_handle();

                        let should_minimize = app_handle
                            .store("settings.dat")
                            .ok()
                            .and_then(|s| s.get("minimizeOnClose"))
                            .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
                            .unwrap_or(true);

                        if should_minimize {
                            #[cfg(not(target_os = "linux"))]
                            {
                                // Windows / macOS: 阻止销毁，隐藏窗口
                                _api.prevent_close();
                                let _ = w.hide();
                            }
                            #[cfg(target_os = "linux")]
                            {
                                // Linux: 不阻止，让窗口彻底物理销毁以避免 GTK 控件失效 Bug
                                println!("Linux: 初始窗口已彻底销毁，准备后台运行");
                            }
                        } else {
                            println!("用户请求退出，正在关闭程序...");
                            log::info!("Exit the application...");
                            app_handle.exit(0);
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_download,
            cancel_download,
            delete_download,
            force_merge,
            get_cpu_info,
            delete_file,
            save_settings,
            check_update,
            save_store_file,
            send_notification_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            // ==========================================
            // 拦截应用级退出事件 (Wayland 强制关闭 / 窗口销毁后的兜底)
            // ==========================================
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // code.is_none() 表示这是由于最后一个窗口被系统销毁触发的隐式退出
                if code.is_none() {
                    let should_minimize = app_handle
                        .store("settings.dat")
                        .ok()
                        .and_then(|s| s.get("minimizeOnClose"))
                        .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
                        .unwrap_or(true);

                    if should_minimize {
                        // 阻止进程死亡，托盘存活！
                        api.prevent_exit();
                        println!("Wayland/Linux 退出拦截：已阻止进程退出，保持托盘后台运行");
                    }
                    // 如果 should_minimize 为 false，这里什么都不做，程序将顺理成章地真正退出。
                }
            }
        });
}

/// 恢复窗口显示
fn restore_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// 获取现有主窗口，如果不存在（被 Linux 销毁了）则重新创建一个
/// 自动从 settings.dat 读取 lastRoute 恢复上次页面
pub fn get_or_create_main_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("main") {
        return Some(window);
    }

    // 窗口已被销毁 — 从 store 读取上次路由，重建窗口
    let last_route = app
        .store("settings.dat")
        .ok()
        .and_then(|s| s.get("lastRoute"))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "dList".to_string());

    let url = format!("index.html#/{}", last_route);
    create_main_window(app, &url)
}

/// 以指定 URL 创建主窗口
pub fn create_main_window(app: &AppHandle, url: &str) -> Option<tauri::WebviewWindow> {
    log::warn!("tray: creating window with url={}", url);

    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(url.into()))
        .title("m3u8下载器")
        .inner_size(1200.0, 750.0)
        .min_inner_size(1200.0, 750.0)
        .center();

    #[cfg(target_os = "macos")]
    {
        use tauri::TitleBarStyle;
        builder = builder
            .title_bar_style(TitleBarStyle::Overlay)
            .decorations(true)
            .shadow(true);
    }

    #[cfg(not(target_os = "macos"))]
    {
        // 恢复原生边框和关闭按钮
        builder = builder.decorations(true);
    }

    match builder.build() {
        Ok(w) => {
            log::info!("tray: window-created label=main");

            // ==========================================
            // 为新复活的窗口重新绑定关闭拦截逻辑
            // ==========================================
            let w_clone = w.clone();
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api: _api, .. } = event {
                    let app_handle = w_clone.app_handle();
                    let should_minimize = app_handle
                        .store("settings.dat")
                        .ok()
                        .and_then(|s| s.get("minimizeOnClose"))
                        .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
                        .unwrap_or(true);

                    if should_minimize {
                        #[cfg(not(target_os = "linux"))]
                        {
                            _api.prevent_close();
                            let _ = w_clone.hide();
                        }
                        #[cfg(target_os = "linux")]
                        {
                            // Linux 下直接让窗口销毁
                            println!("Linux: 重新创建的窗口已销毁");
                        }
                    } else {
                        println!("用户请求退出，正在关闭程序...");
                        log::info!("Exit the application...");
                        app_handle.exit(0);
                    }
                }
            });

            Some(w)
        }
        Err(e) => {
            log::error!("tray: window-create-failed error={}", e);
            None
        }
    }
}

/// 设置并启用系统托盘
fn enable_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{MenuBuilder, MenuItem},
        tray::TrayIconBuilder,
    };

    // 菜单项
    let show_item = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    // 组装菜单
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .separator()
        .item(&settings_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let _tray = TrayIconBuilder::with_id("tray")
        .show_menu_on_left_click(false)
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("m3u8视频下载器")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| match event {
            // 左键单击托盘图标
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } => {
                let app = tray.app_handle();
                // 使用重建逻辑
                if let Some(window) = get_or_create_main_window(app) {
                    restore_window(&window);
                }
            }
            _ => {}
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                #[cfg(target_os = "macos")]
                {
                    use tauri::ActivationPolicy;
                    let _ = app.set_activation_policy(ActivationPolicy::Regular);
                }
                if let Some(window) = get_or_create_main_window(app) {
                    restore_window(&window);
                }
            }
            "settings" => {
                #[cfg(target_os = "macos")]
                {
                    use tauri::ActivationPolicy;
                    let _ = app.set_activation_policy(ActivationPolicy::Regular);
                }

                // 窗口已存在时直接发送事件；不存在时以设置路由重建
                if app.get_webview_window("main").is_some() {
                    if let Some(window) = get_or_create_main_window(app) {
                        let _ = window.emit("open_settings", "");
                        restore_window(&window);
                    }
                } else {
                    let url = "index.html#/setting";
                    if let Some(window) = create_main_window(app, url) {
                        restore_window(&window);
                    }
                }
            }
            "quit" => {
                log::info!("Exit the application...");
                app.exit(0);
            }
            _ => {
                println!("menu item {:?} not handled", event.id);
            }
        })
        .build(app)?;

    Ok(())
}
