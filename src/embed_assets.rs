use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env::var,
    fs::File,
    io::BufReader,
    io::Cursor,
    path::{Component, Path, PathBuf},
};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct AssetKey(String);

impl From<AssetKey> for String {
    fn from(key: AssetKey) -> Self {
        key.0
    }
}

impl AsRef<str> for AssetKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<P: AsRef<Path>> From<P> for AssetKey {
    fn from(path: P) -> Self {
        // TODO: change this to utilize `Cow` to prevent allocating an intermediate `PathBuf` when not necessary
        let path = path.as_ref().to_owned();

        // add in root to mimic how it is used from a server url
        let path = if path.has_root() {
            path
        } else {
            Path::new(&Component::RootDir).join(path)
        };

        let buf = if cfg!(windows) {
            let mut buf = String::new();
            for component in path.components() {
                match component {
                    Component::RootDir => buf.push('/'),
                    Component::CurDir => buf.push_str("./"),
                    Component::ParentDir => buf.push_str("../"),
                    Component::Prefix(prefix) => {
                        buf.push_str(&prefix.as_os_str().to_string_lossy())
                    }
                    Component::Normal(s) => {
                        buf.push_str(&s.to_string_lossy());
                        buf.push('/')
                    }
                }
            }

            // remove the last slash
            if buf != "/" {
                buf.pop();
            }

            buf
        } else {
            path.to_string_lossy().to_string()
        };

        AssetKey(buf)
    }
}

/// (key, (original filepath, compressed bytes))
type Asset = (AssetKey, (String, Vec<u8>));

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddedAssets(HashMap<AssetKey, (String, Vec<u8>)>);

/// All possible errors while reading and compressing an [`EmbeddedAssets`] directory
#[derive(Debug, Error)]
pub enum EmbeddedAssetsError {
    #[error("failed to read asset at {path} because {error}")]
    AssetRead {
        path: PathBuf,
        error: std::io::Error,
    },

    #[error("failed to write asset from {path} to Vec<u8> because {error}")]
    AssetWrite {
        path: PathBuf,
        error: std::io::Error,
    },

    #[error("invalid prefix {prefix} used while including path {path}")]
    PrefixInvalid { prefix: PathBuf, path: PathBuf },

    #[error("failed to walk directory {path} because {error}")]
    Walkdir {
        path: PathBuf,
        error: walkdir::Error,
    },
}

pub trait Assets {
    /// Get the content of the passed [`AssetKey`].
    fn get(&self, key: &str) -> Option<Vec<u8>>;
}

impl Assets for EmbeddedAssets {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        println!("SEARCH FOR {}", key);
        let (_, vec) = self.0.get(&AssetKey::from(String::from(key)))?;
        let decoded_vec = zstd::decode_all(Cursor::new(vec)).unwrap();
        Some(decoded_vec)
    }
}

impl EmbeddedAssets {
    /// Compress a directory of assets, ready to be generated into a [`tauri_api::assets::Assets`].
    pub fn new(path: &Path) -> Result<Self, EmbeddedAssetsError> {
        WalkDir::new(&path)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| match entry {
                // we only serve files, not directory listings
                Ok(entry) if entry.file_type().is_dir() => None,

                // compress all files encountered
                Ok(entry) => Some(Self::compress_file(path, entry.path())),

                // pass down error through filter to fail when encountering any error
                Err(error) => Some(Err(EmbeddedAssetsError::Walkdir {
                    path: path.to_owned(),
                    error,
                })),
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Use highest compression level for release, the fastest one for everything else
    fn compression_level() -> i32 {
        match var("PROFILE").as_ref().map(String::as_str) {
            Ok("release") => 22,
            _ => -5,
        }
    }

    /// Compress a file and spit out the information in a [`HashMap`] friendly form.
    fn compress_file(prefix: &Path, path: &Path) -> Result<Asset, EmbeddedAssetsError> {
        let reader = File::open(&path).map(BufReader::new).map_err(|error| {
            EmbeddedAssetsError::AssetRead {
                path: path.to_owned(),
                error,
            }
        })?;

        // entirely read compressed asset into bytes
        let bytes = zstd::encode_all(reader, Self::compression_level()).map_err(|error| {
            EmbeddedAssetsError::AssetWrite {
                path: path.to_owned(),
                error,
            }
        })?;

        // get a key to the asset path without the asset directory prefix
        let key = path
            .strip_prefix(prefix)
            .map(AssetKey::from) // format the path for use in assets
            .map_err(|_| EmbeddedAssetsError::PrefixInvalid {
                prefix: prefix.to_owned(),
                path: path.to_owned(),
            })?;

        Ok((key, (path.display().to_string(), bytes)))
    }
}
