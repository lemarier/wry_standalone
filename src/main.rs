pub use anyhow::Result;
use clap::{App, Arg};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::prelude::*;
use wry::{Application, Attributes, CustomProtocol, RpcRequest, RpcResponse, WindowProxy};

mod embed_assets;
mod standalone;

#[derive(Debug, Serialize, Deserialize)]
struct MessageParameters {
    message: String,
}

fn main() -> Result<()> {
    let standalone_res = match standalone::extract_standalone() {
        Ok(Some((metadata, assets))) => standalone::run(assets, metadata),
        Ok(None) => Ok(()),
        Err(err) => Err(err),
    };
    if let Err(_err) = standalone_res {
        std::process::exit(1);
    }

    let mut app = Application::new()?;
    let matches = App::new("wry")
        .subcommand(
            App::new("run")
                .about("Run application")
                .arg(Arg::with_name("html-file").required(true)),
        )
        .subcommand(
            App::new("compile")
                .about("Compile application binary")
                .arg(Arg::with_name("html-file").required(true)),
        )
        .get_matches();

    if let Some(run_matches) = matches.subcommand_matches("run") {
        // we should have a path like
        // ./examples/project1/src/index.html
        let root_entry_point = std::path::PathBuf::from(run_matches.value_of("html-file").unwrap());

        // get file name like index.html
        let root_file_name = root_entry_point.file_name().unwrap().to_str().unwrap();

        // our project path source should be
        // ./examples/project1/src/
        let root_path = std::fs::canonicalize(root_entry_point.clone().parent().unwrap())?;

        let attributes = Attributes {
            url: Some(format!("wry://{}", root_file_name)),
            title: String::from("Hello World!"),
            ..Default::default()
        };

        let handler = Box::new(|proxy: WindowProxy, mut req: RpcRequest| {
            let mut response = None;
            println!("{:?}", req.method);
            if &req.method == "fullscreen" {
                if let Some(params) = req.params.take() {
                    if let Ok(mut args) = serde_json::from_value::<Vec<bool>>(params) {
                        if !args.is_empty() {
                            let flag = args.swap_remove(0);
                            // NOTE: in the real world we need to reply with an error
                            let _ = proxy.set_fullscreen(flag);
                        };
                        response = Some(RpcResponse::new_result(req.id.take(), None));
                    }
                }
            } else if &req.method == "send-parameters" {
                if let Some(params) = req.params.take() {
                    if let Ok(mut args) = serde_json::from_value::<Vec<MessageParameters>>(params) {
                        let result = if !args.is_empty() {
                            let msg = args.swap_remove(0);
                            Some(Value::String(format!("Hello, {}!", msg.message)))
                        } else {
                            // NOTE: in the real-world we should send an error response here!
                            None
                        };
                        // Must always send a response as this is a `call()`
                        response = Some(RpcResponse::new_result(req.id.take(), result));
                    }
                }
            }

            response
        });

        fn load_local_file(
            file: &str,
            root_file_name: &str,
            root: std::path::PathBuf,
        ) -> Result<Vec<u8>> {
            let trimed_file = file.replace(&format!("{}/", root_file_name), "");
            let mut file = std::fs::File::open(std::fs::canonicalize(root.join(trimed_file))?)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            Ok(buffer)
        }

        let custom_protocol = CustomProtocol {
            name: "wry".into(),
            handler: Box::new(move |a| {
                load_local_file(
                    &a.replace("wry://", ""),
                    root_entry_point.file_name().unwrap().to_str().unwrap(),
                    root_path.clone(),
                )
                .map_err(|_| wry::Error::InitScriptError)
            }),
        };

        app.add_window_with_configs(attributes, Some(handler), Some(custom_protocol), None)?;
        app.run();

        return Ok(());
    } else if let Some(build_matches) = matches.subcommand_matches("compile") {
        // we should have a path like
        // ./examples/project1/src/index.html
        let root_entry_point =
            std::path::PathBuf::from(build_matches.value_of("html-file").unwrap());

        // our project path source should be
        // ./examples/project1/src/
        let root_path = std::fs::canonicalize(root_entry_point.parent().unwrap())?;

        // embed all assets
        let assets = embed_assets::EmbeddedAssets::new(&root_path)?;

        standalone::compile_command(&assets, None, None)?;
    }

    Ok(())
}
