use clap::{Arg, ArgAction, command, value_parser};
use tokio_util::sync::CancellationToken;

mod app;
mod convert;
mod download;
mod pre_process;
mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("serve") || std::env::args().len() == 1 {
        return server::serve().await;
    }

    let matches = command!()
        .version(env!("CARGO_PKG_VERSION"))
        .author("Ricky1911")
        .about("下载清华教参平台电子书。无参数运行时启动浏览器服务。")
        .arg(
            Arg::new("url")
                .required(true)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("token")
                .required(true)
                .short('t')
                .long("token")
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("thread_number")
                .short('n')
                .value_parser(value_parser!(u8).range(1..17))
                .default_value("4"),
        )
        .arg(
            Arg::new("quality")
                .short('q')
                .value_parser(value_parser!(u8).range(3..11))
                .default_value("10"),
        )
        .arg(
            Arg::new("del_img")
                .short('d')
                .long("del-img")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("auto_resize")
                .short('r')
                .long("auto-resize")
                .action(ArgAction::SetTrue),
        )
        .get_matches();

    let cancel = CancellationToken::new();
    let options = app::DownloadOptions {
        url: matches.get_one::<String>("url").unwrap().clone(),
        token: matches.get_one::<String>("token").unwrap().clone(),
        thread_number: *matches.get_one::<u8>("thread_number").unwrap() as usize,
        quality: *matches.get_one::<u8>("quality").unwrap() as u32,
        del_img: *matches.get_one::<bool>("del_img").unwrap(),
        auto_resize: *matches.get_one::<bool>("auto_resize").unwrap(),
        output_root: std::env::current_dir()?.join("downloads"),
    };
    let result = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
            Err("任务已取消".to_string())
        },
        result = app::run_download(options, cancel.clone(), None, None, None) => result,
    };
    match result {
        Ok(path) => {
            println!("PDF 已生成：{}", path.display());
            Ok(())
        }
        Err(error) => Err(std::io::Error::other(error).into()),
    }
}
