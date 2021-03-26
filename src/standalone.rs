pub const MAGIC_TRAILER: &[u8; 8] = b"t4ur1wry";

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;
use std::convert::TryInto;
use std::env::current_exe;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use wry::{Application, Attributes, CustomProtocol};

use crate::embed_assets::{Assets, EmbeddedAssets};

#[derive(Deserialize, Serialize)]
pub struct Metadata {}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn compile_command(
    assets: &EmbeddedAssets,
    output: Option<std::path::PathBuf>,
    target: Option<String>,
) -> crate::Result<()> {
    let original_binary = get_base_binary()?;

    let final_bin = create_standalone_binary(original_binary, assets)?;

    let output = output
        .or_else(|| Some(std::path::PathBuf::from("compiled-bin-test")))
        .unwrap();

    write_standalone_binary(output, target, final_bin)?;

    Ok(())
}

fn create_standalone_binary(
    mut original_bin: Vec<u8>,
    assets: &EmbeddedAssets,
) -> crate::Result<Vec<u8>> {
    let mut source_code = serde_json::to_string(&assets)?.as_bytes().to_vec();

    let metadata = Metadata {};
    let mut metadata = serde_json::to_string(&metadata)?.as_bytes().to_vec();

    let bundle_pos = original_bin.len();
    let metadata_pos = bundle_pos + source_code.len();
    let mut trailer = MAGIC_TRAILER.to_vec();
    trailer.write_all(&bundle_pos.to_be_bytes())?;
    trailer.write_all(&metadata_pos.to_be_bytes())?;

    let mut final_bin = Vec::with_capacity(original_bin.len() + source_code.len() + trailer.len());

    final_bin.append(&mut original_bin);
    final_bin.append(&mut source_code);
    final_bin.append(&mut metadata);
    final_bin.append(&mut trailer);

    Ok(final_bin)
}

fn write_standalone_binary(
    output: std::path::PathBuf,
    target: Option<String>,
    final_bin: Vec<u8>,
) -> crate::Result<()> {
    let output = match target {
        Some(target) => {
            if target.contains("windows") {
                std::path::PathBuf::from(output.display().to_string() + ".exe")
            } else {
                output
            }
        }
        None => {
            if cfg!(windows) && output.extension().unwrap_or_default() != "exe" {
                std::path::PathBuf::from(output.display().to_string() + ".exe")
            } else {
                output
            }
        }
    };

    if output.exists() {
        // If the output is a directory, throw error
        if output.is_dir() {
            anyhow::bail!("Could not compile: {:?} is a directory.", &output);
        }

        // Make sure we don't overwrite any file not created by Deno compiler.
        // Check for magic trailer in last 24 bytes.
        let mut has_trailer = false;
        let mut output_file = File::open(&output)?;
        // This seek may fail because the file is too small to possibly be
        // `deno compile` output.
        if output_file.seek(SeekFrom::End(-24)).is_ok() {
            let mut trailer = [0; 24];
            output_file.read_exact(&mut trailer)?;
            let (magic_trailer, _) = trailer.split_at(8);
            has_trailer = magic_trailer == MAGIC_TRAILER;
        }
        if !has_trailer {
            anyhow::bail!("Could not compile: cannot overwrite {:?}.", &output);
        }
    }
    std::fs::write(&output, final_bin)?;
    #[cfg(unix)]
    {
        let perms = std::fs::Permissions::from_mode(0o777);
        std::fs::set_permissions(output, perms)?;
    }

    Ok(())
}

pub fn extract_standalone() -> crate::Result<Option<(Metadata, EmbeddedAssets)>> {
    let current_exe_path = current_exe()?;

    let mut current_exe = File::open(current_exe_path)?;
    let trailer_pos = current_exe.seek(SeekFrom::End(-24))?;
    let mut trailer = [0; 24];
    current_exe.read_exact(&mut trailer)?;
    let (magic_trailer, rest) = trailer.split_at(8);
    if magic_trailer != MAGIC_TRAILER {
        return Ok(None);
    }

    let (bundle_pos, rest) = rest.split_at(8);
    let metadata_pos = rest;
    let bundle_pos = u64_from_bytes(bundle_pos)?;
    let metadata_pos = u64_from_bytes(metadata_pos)?;
    let bundle_len = metadata_pos - bundle_pos;
    let metadata_len = trailer_pos - metadata_pos;
    current_exe.seek(SeekFrom::Start(bundle_pos))?;

    let bundle = read_string_slice(&mut current_exe, bundle_pos, bundle_len)
        .context("Failed to read source bundle from the current executable")?;

    let metadata = read_string_slice(&mut current_exe, metadata_pos, metadata_len)
        .context("Failed to read metadata from the current executable")?;


    let metadata: Metadata = serde_json::from_str(&metadata).unwrap();
    let assets: EmbeddedAssets = serde_json::from_str(&bundle).unwrap();

    Ok(Some((metadata, assets)))
}

pub fn run(assets: EmbeddedAssets, metadata: Metadata) -> crate::Result<()> {
    //println!("SOURCE: {}", source_code);
    let entry_point_file = "index.html";
    // launch in standalone mode with custom resolver with our embed assets
    let mut app = Application::new()?;

    fn load_local_file(
        file: &str,
        root_file_name: &str,
        assets: &EmbeddedAssets,
    ) -> crate::Result<Vec<u8>> {
        let trimed_file = file.replace(&format!("{}/", root_file_name), "");
        Ok(assets.get(trimed_file.as_str()).unwrap())
    }

    let custom_protocol = CustomProtocol {
        name: "wry".into(),
        handler: Box::new(move |a| {
            load_local_file(&a.replace("wry://", ""), entry_point_file, &assets)
                .map_err(|_| wry::Error::InitScriptError)
        }),
    };

    let attributes = Attributes {
        url: Some(format!("wry://{}", entry_point_file)),
        title: String::from("Hello World!"),
        ..Default::default()
    };

    app.add_window_with_configs(attributes, None, Some(custom_protocol), None)?;
    app.run();

    Ok(())
}

fn get_base_binary() -> crate::Result<Vec<u8>> {
    // return current bin for now
    let path = std::env::current_exe()?;
    Ok(std::fs::read(path)?)
}

fn u64_from_bytes(arr: &[u8]) -> crate::Result<u64> {
    let fixed_arr: &[u8; 8] = arr
        .try_into()
        .context("Failed to convert the buffer into a fixed-size array")?;
    Ok(u64::from_be_bytes(*fixed_arr))
}

fn read_string_slice(file: &mut File, pos: u64, len: u64) -> crate::Result<String> {
    let mut string = String::new();
    file.seek(SeekFrom::Start(pos))?;
    file.take(len).read_to_string(&mut string)?;
    // TODO: check amount of bytes read
    Ok(string)
}
