use std::path::PathBuf;
use std::process::exit;

use subvault::workbench::{run_standalone, WorkbenchOptions, DEFAULT_WORKBENCH_PORT};

fn main() {
    let mut port: u16 = DEFAULT_WORKBENCH_PORT;
    let mut open_browser = true;
    let mut initial_vault: Option<PathBuf> = None;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-browser" | "--no-open" => {
                open_browser = false;
                i += 1;
            }
            "--port" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("错误：--port 缺少端口号参数（示例：--port 18421）");
                    exit(2);
                };
                match val.parse::<u16>() {
                    Ok(p) => port = p,
                    Err(_) => {
                        eprintln!("错误：无效的端口号 `{val}`，请输入 0–65535 的整数");
                        exit(2);
                    }
                }
                i += 2;
            }
            "--vault" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("错误：--vault 缺少账本路径参数");
                    exit(2);
                };
                initial_vault = Some(PathBuf::from(val));
                i += 2;
            }
            "-V" | "--version" => {
                println!("subvault-app {}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            "-h" | "--help" => {
                println!(
                    "SubVault 本地订阅与账单工作台\n\n用法:\n  subvault-app [--vault 路径] [--port 端口] [--no-browser] [--version]\n\n说明:\n  双击启动后会在本机 127.0.0.1 开启隐私工作台并自动打开界面，所有数据仅存于本地磁盘，不联网。"
                );
                exit(0);
            }
            other => {
                eprintln!("错误：未知参数 `{other}`。运行 `subvault-app --help` 查看用法。");
                exit(2);
            }
        }
    }

    if let Err(err) = run_standalone(WorkbenchOptions {
        port,
        open_browser,
        initial_vault,
    }) {
        eprintln!("SubVault 启动失败: {err}");
        exit(1);
    }
}
