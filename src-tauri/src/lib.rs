//! WXdown 桌面端：把 `wx-core` 的能力通过命令暴露给界面。

mod commands;
mod naming;
mod state;
mod types;

use tauri::{Theme, WebviewUrl, WebviewWindowBuilder};

const BG_DARK: tauri::window::Color = tauri::window::Color(15, 16, 17, 255);
const BG_LIGHT: tauri::window::Color = tauri::window::Color(245, 243, 244, 255);

pub fn run() {
    // 取数窗口常年是隐藏的（登录后默认收起、取数时也会先收起），而 WebView2 对
    // 隐藏/被遮挡的页面会挂起渲染、把定时器降到 1 次/分钟 —— 实测表现为：导航指令
    // 发出后新页面根本没落地，取数一直读到旧页面，还把我们引向"token 失效"的误判。
    // 这几个开关让隐藏窗口照常跑（WebView2 环境创建前设置才生效）。
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-background-timer-throttling \
             --disable-backgrounding-occluded-windows \
             --disable-renderer-backgrounding \
             --disable-features=CalculateNativeWinOcclusion",
        );
    }

    let state = state::AppState::new().expect("初始化应用状态失败");
    // 启动前读出主题，注入初始化脚本：页面首帧即为正确配色，杜绝启动闪白
    let theme_mode = state.settings().theme.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // 自动更新：检测/下载/安装走 GitHub Releases 签名更新包；
        // process 插件用于安装完成后 relaunch
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        // 主窗口一关就退出应用：取数窗口是隐藏的，如果等"所有窗口关闭"才退出，
        // 用户关掉主窗口后进程会带着隐藏窗口继续留在后台（而且还占着 exe 不让更新）
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) && window.label() == "main" {
                use tauri::Manager as _;
                window.app_handle().exit(0);
            }
        })
        // 兜底：页面加载完成即显示窗口（正常路径是前端挂载后主动调用）
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = window.show();
            }
        })
        .setup(move |app| {
            // 初始化脚本在页面任何脚本执行前运行：跟随系统时直接写 "system"，
            // 由 CSS 的 prefers-color-scheme media query 在首帧完成配色。
            let init_theme = match theme_mode.as_str() {
                "dark" => "dark",
                "light" => "light",
                _ => "system",
            };
            // WebView2 执行初始化脚本时 document.documentElement 可能还是 null，
            // 直接写 dataset 会抛 TypeError、属性永远落不上（BILIdown 踩过），
            // 改成「<html> 一出现就写」。
            let init_script = format!(
                r##"(function () {{
  var mode = "{init_theme}";
  var tries = 0;
  function apply() {{
    var el = document.documentElement;
    if (!el) return false;
    el.dataset.theme = mode;
    return true;
  }}
  if (!apply()) {{
    var timer = setInterval(function () {{
      if (apply() || ++tries > 1000) clearInterval(timer);
    }}, 0);
  }}
}})();"##
            );
            // 窗口先隐藏，前端渲染完成后由 main.js 调 JS API 的 show() 显示
            //（彻底避免「先见白底/旧底色、再见内容」的启动闪烁）。
            let window = WebviewWindowBuilder::new(app.handle(), "main", WebviewUrl::default())
                .title("WXdown")
                .inner_size(1100.0, 740.0)
                .min_inner_size(900.0, 600.0)
                .resizable(true)
                .center()
                .decorations(false)
                .visible(false)
                .initialization_script(init_script)
                .build()?;

            // 原生窗口底色：三种模式都要设。窗口可能在页面首帧之前就被系统显示出来，
            // 这时能看到的就是这层底色——不设就是 WebView2 的白，深色下即一块白色空壳。
            let resolved_dark = match theme_mode.as_str() {
                "dark" => true,
                "light" => false,
                _ => matches!(window.theme(), Ok(Theme::Dark)),
            };
            let _ =
                window.set_background_color(Some(if resolved_dark { BG_DARK } else { BG_LIGHT }));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::app_settings,
            commands::update_settings,
            commands::probe_source,
            commands::probe_more,
            commands::start_download,
            commands::cancel_download,
            commands::web_login_open,
            commands::web_login_cookies,
            commands::web_login_watch,
            commands::web_login_close,
            commands::logout,
            commands::choose_output_dir,
            commands::open_path,
            commands::naming_variables,
            commands::ui_log,
            commands::resume_pending,
            commands::library_folders,
            commands::preview_naming,
            commands::preview_names,
            commands::cleanup_temp,
            commands::export_diagnostics,
            commands::check_updates,
            commands::reveal_path,
            commands::pause_download,
            commands::resume_download,
            commands::pause_all_downloads,
            commands::resume_all_downloads,
            commands::rename_downloaded,
        ])
        .run(tauri::generate_context!())
        .expect("WXdown 启动失败");
}
