//! MITM 链路自测：起代理 → 打印嗅探事件 → 等 30 秒退出。
//! 配合 curl 验证（不碰系统代理）：
//!   cargo run -p wx-sniffer --example mitm_test
//!   curl -x http://127.0.0.1:8899 --cacert <数据目录>/proxy-ca.crt \
//!        https://mp.weixin.qq.com/s/xxxx -o /dev/null
//!   curl -x http://127.0.0.1:8899 --cacert <数据目录>/proxy-ca.crt \
//!        "https://cdn.example.com/video/1a2b3c4d5e6f" -o /dev/null   # 规则命中(上游会 404,无妨)

use std::time::Duration;

#[tokio::main]
async fn main() {
    let _ = std::fs::write(".dev/mitm-marker.txt", b"started
");
    let dir = std::env::temp_dir().join("wxdown-mitm-test");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (_stop, ready) = wx_sniffer::run_proxy(8899, dir.clone(), tx).expect("启动代理");
    ready.await.expect("等待监听").expect("监听失败");
    println!("代理已监听 127.0.0.1:8899");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    println!("CA 证书：{}/proxy-ca.crt", dir.display());
    println!("等 30 秒收事件…");
    let _ = std::fs::write(".dev/mitm-events.log", "[loop] 进入接收循环
");
    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(".dev/mitm-events.log") {
                    let _ = writeln!(f, "[loop] 30s 到点退出");
                }
                break;
            }
            event = rx.recv() => {
                use std::io::Write as _;
                let line = match event {
                    Some(wx_sniffer::SniffEvent::Media { kind, url, host }) => {
                        format!("[Media] {kind} host={host} url={url}
")
                    }
                    Some(wx_sniffer::SniffEvent::Profile { biz, action, cookies, .. }) => {
                        format!("[Profile] biz={biz} action={action} cookies={} 个
", cookies.len())
                    }
                    None => "[loop] 通道关闭
".to_string(),
                };
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(".dev/mitm-events.log") {
                    let _ = write!(f, "{line}");
                }
                if line.starts_with("[loop]") { break; }
            }
        }
    }
    let _ = std::fs::write(".dev/mitm-marker.txt", "done
");
}
