pub use anyhow::Result;
use clap::{App, Arg};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap, io::prelude::*, path::PathBuf};

use deno_core::error::AnyError;
use deno_core::json_op_sync;
use deno_core::FsModuleLoader;
use deno_core::{resolve_path, resolve_url};
use deno_runtime::permissions::Permissions;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use std::rc::Rc;
use std::sync::Arc;

use deno_core::error::anyhow;
use wry::webview::{RpcRequest, WebView, WebViewBuilder};

use winit::platform::run_return::EventLoopExtRunReturn;
use winit::{
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

mod embed_assets;
mod event;
mod helpers;
mod standalone;

use serde_json::json;

use embed_assets::{Assets, EmbeddedAssets};
use event::Event;
use helpers::WebViewStatus;

thread_local! {
  static INDEX: RefCell<u64> = RefCell::new(0);
  static EVENT_LOOP: RefCell<EventLoop<()>> = RefCell::new(EventLoop::new());
  static WEBVIEW_MAP: RefCell<HashMap<u64, WebView>> = RefCell::new(HashMap::new());
  static WEBVIEW_STATUS: RefCell<HashMap<u64, WebViewStatus>> = RefCell::new(HashMap::new());
  static STACK_MAP: RefCell<HashMap<u64, Vec<event::Event>>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageParameters {
    message: String,
}

#[derive(Deserialize, Serialize)]
struct ResourceId {
    rid: u32,
}

fn get_error_class_name(e: &AnyError) -> &'static str {
    deno_runtime::errors::get_error_class_name(e).unwrap_or("Error")
}

#[tokio::main]
async fn main() -> Result<()> {
    let standalone_res = match standalone::extract_standalone() {
        Ok(Some((metadata, assets))) => standalone::run(assets, metadata).await,
        Ok(None) => Ok(()),
        Err(err) => Err(err),
    };

    if let Err(_err) = standalone_res {
        std::process::exit(1);
    }

    let matches = App::new("wry")
        .subcommand(
            App::new("run")
                .about("Run application")
                .arg(Arg::with_name("js-file").required(true)),
        )
        .subcommand(
            App::new("compile")
                .about("Compile application binary")
                .arg(Arg::with_name("js-file").required(true)),
        )
        .get_matches();

    if let Some(run_matches) = matches.subcommand_matches("run") {
        // we should have a path like
        // ./examples/project1/src/index.html
        let entry_point = run_matches.value_of("js-file").unwrap();
        run_wry(entry_point, None).await?
    } else if let Some(build_matches) = matches.subcommand_matches("compile") {
        // we should have a path like
        // ./examples/project1/src/index.html
        let root_entry_point = std::path::PathBuf::from(build_matches.value_of("js-file").unwrap());
        // our project path source should be
        // ./examples/project1/src/
        let root_path = std::fs::canonicalize(root_entry_point.parent().unwrap())?;

        // embed all assets
        let assets = EmbeddedAssets::new(&root_path)?;

        standalone::compile_command(&assets, None, None)?;
    }

    Ok(())
}

pub async fn run_wry(main_module_path: &str, assets: Option<EmbeddedAssets>) -> Result<()> {
    let module_loader: Rc<dyn deno_core::ModuleLoader>;
    let main_module_pathbuf = PathBuf::from(main_module_path);
    let main_module: deno_core::ModuleSpecifier;

    // our project path source should be
    // ./examples/project1/src/
    let mut root_path = None;

    if let Some(assets) = assets.clone() {
        println!("GET SOURCE");
        let source_code = assets
            .get(main_module_path)
            .expect("Unable to extract source code");
        println!("SOURCE {}", String::from_utf8(source_code.clone())?);
        module_loader = Rc::new(standalone::EmbeddedModuleLoader(String::from_utf8(
            source_code,
        )?));
        main_module = resolve_url(standalone::SPECIFIER)?;
    } else {
        root_path = Some(std::fs::canonicalize(
            main_module_pathbuf.clone().parent().unwrap(),
        )?);
        module_loader = Rc::new(FsModuleLoader);
        main_module = resolve_url(standalone::SPECIFIER)?;
    }

    fn load_local_file(
        file: &str,
        root_file_name: &str,
        root: Option<std::path::PathBuf>,
        assets: Option<EmbeddedAssets>,
    ) -> Result<Vec<u8>> {
        if let Some(assets) = assets {
            let mut file_name = file;
            let trimmed = file_name.replace("./", "");
            if file_name.starts_with("./") {
                file_name = &trimmed;
            }
            let trimed_file = file_name.replace(&format!("{}/", root_file_name), "");
            Ok(assets.get(trimed_file.as_str()).unwrap())
        } else {
            let trimed_file = file.replace(&format!("{}/", root_file_name), "");
            let mut file = std::fs::File::open(std::fs::canonicalize(
                root.expect("No root path found").join(trimed_file),
            )?)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            Ok(buffer)
        }
    }

    let create_web_worker_cb = Arc::new(|_| {
        todo!("Web workers are not supported");
    });

    let options = WorkerOptions {
        apply_source_maps: false,
        args: vec![],
        debug_flag: true,
        unstable: true,
        ca_data: None,
        user_agent: "hello_runtime".to_string(),
        seed: None,
        js_error_create_fn: None,
        create_web_worker_cb,
        attach_inspector: false,
        maybe_inspector_server: None,
        should_break_on_first_statement: false,
        module_loader,
        runtime_version: "x".to_string(),
        ts_version: "x".to_string(),
        no_color: false,
        get_error_class_fn: Some(&get_error_class_name),
        location: None,
    };

    let permissions = Permissions::allow_all();

    let mut worker = MainWorker::from_options(main_module.clone(), permissions, &options);

    // return pending Events
    worker.js_runtime.register_op(
        "wry_step",
        json_op_sync(move |_state, json: Value, _zero_copy| {
            let id = json["id"].as_u64().unwrap();
            STACK_MAP.with(|cell| {
                let mut stack_map = cell.borrow_mut();
                if let Some(stack) = stack_map.get_mut(&id) {
                    let ret = stack.clone();
                    stack.clear();
                    Ok(json!(ret))
                } else {
                    Err(anyhow!("Could not find stack with id: {}", id))
                }
            })
        }),
    );

    // this is an ugly workaround -- we need a better way to handle this
    // this loop record event from winit and if needed inject them inside our STACK_MAP
    // to be pulled with wry_step -- we also dispatch event to our webview if we got a resize
    // or if we got a close event
    worker.js_runtime.register_op("wry_loop", json_op_sync(move |_state, json: Value, _zero_copy| {
         let id = json["id"].as_u64().unwrap();
         let mut should_stop_loop = false;
         EVENT_LOOP.with(|cell| {
             let event_loop = &mut *cell.borrow_mut();
             event_loop.run_return(|event, _, control_flow| {
                 *control_flow = ControlFlow::Exit;
 
                 WEBVIEW_MAP.with(|cell| {
                     let webview_map = cell.borrow();
 
                     if let Some(webview) = webview_map.get(&id) {
                         match event {
                             winit::event::Event::WindowEvent {
                                 event: winit::event::WindowEvent::CloseRequested,
                                 ..
                             } => {
                                 should_stop_loop = true;
                             }
                             winit::event::Event::WindowEvent {
                                 event: winit::event::WindowEvent::Resized(_),
                                 ..
                             } => {
                                 webview.resize().unwrap();
                             }
                             winit::event::Event::MainEventsCleared => {
                                 webview.window().request_redraw();
                             }
                             winit::event::Event::RedrawRequested(_) => {}
                             _ => (),
                         };
 
                         // set this webview as WindowCreated if needed
                         WEBVIEW_STATUS.with(|cell| {
                             let mut status_map = cell.borrow_mut();
                             if let Some(status) = status_map.get_mut(&id) {
                                 match status {
                                     &mut WebViewStatus::Initialized => {
                                         *status = WebViewStatus::WindowCreated;
                                         STACK_MAP.with(|cell| {
 
                                   let mut stack_map = cell.borrow_mut();
                                   if let Some(stack) = stack_map.get_mut(&id) {
                                       stack.push(Event::WindowCreated);
                                   } else {
                                       panic!("Could not find stack with id {} to push onto stack", id);
                                   }
                               });
                                     }
                                     _ => {}
                                 };
                             }
                         });
                     }
                 });
 
                 // add our event inside our stack to be pulled by the next step
                 STACK_MAP.with(|cell| {
                     let mut stack_map = cell.borrow_mut();
                     if let Some(stack) = stack_map.get_mut(&id) {
                         let wry_event = Event::from(event);
                         match wry_event {
                             Event::Undefined => {}
                             _ => {
                                 stack.push(wry_event);
                             }
                         };
                     } else {
                         panic!("Could not find stack with id {} to push onto stack", id);
                     }
                 });
             });
         });
 
         Ok(json!(should_stop_loop))
       }));

    worker.js_runtime.register_op(
        "wry_new",
        json_op_sync(move |_state, json: Value, _zero_copy| {
            let url = json["url"].as_str().unwrap();
            let root_entry_point = main_module_pathbuf.clone();
            let root_path = root_path.clone();
            let assets = assets.clone();

            println!("{}", url);

            let mut id = 0;
            INDEX.with(|cell| {
                id = cell.replace_with(|&mut i| i + 1);
            });

            WEBVIEW_MAP.with(|cell| {
                let mut webviews = cell.borrow_mut();
                EVENT_LOOP.with(|cell| {
                    let event_loop = cell.borrow();
                    let window = Window::new(&event_loop)?;

                    let webview =
                        WebViewBuilder::new(window)
                            .unwrap()
                            // inject a DOMContentLoaded listener to send a RPC request
                            .initialize_script(
                                format!(
                                    r#"
                           {dom_loader}
                         "#,
                                    dom_loader = include_str!("scripts/dom_loader.js"),
                                )
                                .as_str(),
                            )
                            .load_url(format!("wry://{}", url).as_str())?
                            .set_rpc_handler(Box::new(move |req: RpcRequest| {
                                // this is a sample RPC test to check if we can get everything to work together
                                let response = None;
                                if &req.method == "domContentLoaded" {
                                    STACK_MAP.with(|cell| {
 
                           let mut stack_map = cell.borrow_mut();
                           if let Some(stack) = stack_map.get_mut(&id) {
                               stack.push(Event::DomContentLoaded);
                           } else {
                               panic!("Could not find stack with id {} to push onto stack", id);
                           }
                       });
                                }
                                response
                            }))
                            .register_protocol(
                                "wry".into(),
                                Box::new(move |a: &str| {
                                    load_local_file(
                                        &a.replace("wry://", ""),
                                        root_entry_point.file_name().unwrap().to_str().unwrap(),
                                        root_path.clone(),
                                        assets.clone(),
                                    )
                                    .map_err(|_| wry::Error::InitScriptError)
                                }),
                            )
                            .build()?;

                    webviews.insert(id, webview);
                    STACK_MAP.with(|cell| {
                        cell.borrow_mut().insert(id, Vec::new());
                    });

                    // Set status to Initialized
                    // on next loop we will mark this as window created
                    WEBVIEW_STATUS.with(|cell| {
                        cell.borrow_mut().insert(id, WebViewStatus::Initialized);
                    });

                    Ok(json!(id))
                })
            })
        }),
    );

    // inject webview.js
    worker
        .js_runtime
        .execute("<webview>", include_str!("scripts/webview.js"))?;

    worker.bootstrap(&options);
    worker.execute_module(&main_module).await?;
    worker.run_event_loop().await?;

    Ok(())
}
