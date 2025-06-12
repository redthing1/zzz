//! command line interface

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "zzz",
    version,
    about = "zzz: compression multitool",
    long_about = "Create and extract archives in multiple formats (zst, tgz, txz, zip, 7z) with smart file filtering and magic number detection"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// number of threads (0 = auto-detect)
    #[arg(long, global = true, default_value = "0")]
    pub threads: u32,
}

#[derive(Subcommand)]
pub enum Commands {
    /// compress files/directories (supports .zst, .tgz, .txz, .zip, .7z)
    #[command(alias = "c")]
    Compress {
        /// compression level (1-22)
        #[arg(short, long, default_value = "19", value_parser = clap::value_parser!(i32).range(1..=22))]
        level: i32,

        /// output file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// show progress bar
        #[arg(short = 'P', long)]
        progress: bool,

        /// exclude files matching pattern (repeatable)
        #[arg(long)]
        exclude: Vec<String>,

        /// disable built-in garbage file filtering
        #[arg(long)]
        no_default_excludes: bool,

        /// force specific format (zst, tgz, txz, zip, 7z)
        #[arg(short = 'f', long, value_parser = parse_format)]
        format: Option<crate::formats::Format>,

        /// input file or directory
        input: PathBuf,
    },

    /// extract archives (auto-detects format: .zst, .tgz, .txz, .zip, .7z)
    #[command(alias = "x")]
    Extract {
        /// archive file to extract
        archive: PathBuf,

        /// destination directory
        destination: Option<PathBuf>,

        /// extract to specific directory
        #[arg(short = 'C', long)]
        directory: Option<PathBuf>,

        /// overwrite existing files
        #[arg(long)]
        overwrite: bool,
    },

    /// list archive contents
    #[command(alias = "l")]
    List {
        /// archive file to list
        archive: PathBuf,
    },
}

/// Parse format string into Format enum
fn parse_format(s: &str) -> Result<crate::formats::Format, String> {
    match s.to_lowercase().as_str() {
        "zst" | "zstd" => Ok(crate::formats::Format::Zstd),
        "tgz" | "gz" | "gzip" => Ok(crate::formats::Format::Gzip),
        "txz" | "xz" => Ok(crate::formats::Format::Xz),
        "zip" => Ok(crate::formats::Format::Zip),
        "7z" | "sevenz" => Ok(crate::formats::Format::SevenZ),
        _ => Err(format!(
            "unsupported format '{s}'. Supported formats: zst, tgz, txz, zip, 7z"
        )),
    }
}

impl Cli {
    /// get output path for compression, defaulting to input + appropriate extension
    pub fn get_output_path(
        input: &Path,
        output: Option<PathBuf>,
        format: Option<crate::formats::Format>,
    ) -> PathBuf {
        output.unwrap_or_else(|| {
            let mut path = input.to_path_buf();
            let extension = match format {
                Some(f) => f.extension(),
                None => "zst",
            };

            if let Some(filename) = path.file_name() {
                let mut new_filename = filename.to_os_string();
                new_filename.push(".");
                new_filename.push(extension);
                path.set_file_name(new_filename);
            } else {
                path.set_extension(extension);
            }
            path
        })
    }

    /// get extraction directory, defaulting to current directory
    pub fn get_extract_dir(destination: Option<PathBuf>, directory: Option<PathBuf>) -> PathBuf {
        directory
            .or(destination)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
