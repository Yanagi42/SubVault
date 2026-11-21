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
                if let Some(val) = args.get(i + 1) {
                    if let Ok(p) = val.parse::<u16>() {
                        port = p;
                    }
                }
                i += 2;
            }
            "--vault" => {
                if let Some(val) = args.get(i + 1) {
                    initial_vault = Some(PathBuf::from(val));
                }
                i += 2;
            }
            "-h" | "--help" => {
                println!(
                    "SubVault 本地订阅与账单工作台\n\n用法:\n  subvault-app [--vault 路径] [--port 端口] [--no-browser]\n\n说明:\n  双击启动后会在本机 127.0.0.1 开启隐私工作台并自动打开界面，所有数据仅存于本地磁盘，不联网。"
                );
                exit(0);
            }
            _ => {
                i += 1;
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
