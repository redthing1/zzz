//! command line interface

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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

    /// number of threads (0 = auto-detect available parallelism)
    #[arg(short = 'j', long, global = true, default_value = "0")]
    pub threads: u32,
}

#[derive(Subcommand)]
pub enum Commands {
    /// compress files/directories (defaults: preserve perms+timestamps, strip owner+xattrs, skip symlinks)
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
        #[arg(short = 'e', long)]
        exclude: Vec<String>,

        /// preserve extended attributes (xattrs) in tar-based archives
        #[arg(long)]
        preserve_xattrs: bool,

        /// preserve ownership (uid/gid) in tar-based archives
        #[arg(long)]
        preserve_ownership: bool,

        /// follow symlink targets (default: skip symlinks)
        #[arg(long)]
        follow_symlinks: bool,

        /// allow symlink targets outside the input root (requires --follow-symlinks)
        #[arg(long)]
        allow_symlink_escape: bool,

        /// strip filesystem timestamps in archive entries
        #[arg(long)]
        strip_timestamps: bool,

        /// disable built-in garbage file filtering
        #[arg(short = 'E', long)]
        no_default_excludes: bool,

        /// exclude common sensitive files such as .env, .ssh, and key material
        #[arg(long)]
        exclude_sensitive: bool,

        /// force specific format (zst, tgz, txz, zip, 7z)
        #[arg(short = 'f', long, value_parser = parse_format)]
        format: Option<crate::formats::Format>,

        /// overwrite existing output file
        #[arg(short = 'y', long)]
        overwrite: bool,

        /// input files or directories
        #[arg(required = true)]
        input: Vec<PathBuf>,

        #[command(flatten)]
        password: PasswordArgs,

        /// Argon2id cost for encrypted zst: lo=256 MiB, med=1 GiB, hi=8 GiB (default: hi)
        #[arg(long, value_enum)]
        kdf: Option<crate::encryption::KdfPreset>,
    },

    /// extract archives (defaults: preserve perms+timestamps, ignore owner+xattrs)
    #[command(alias = "x")]
    Extract {
        /// archive file to extract
        archive: PathBuf,

        /// destination directory
        destination: Option<PathBuf>,

        /// extract to specific directory
        #[arg(short = 'C', long)]
        directory: Option<PathBuf>,

        /// show progress bar
        #[arg(short = 'P', long)]
        progress: bool,

        /// strip leading path components
        #[arg(long, default_value = "0")]
        strip_components: usize,

        /// preserve extended attributes (xattrs) when extracting tar-based archives
        #[arg(long)]
        preserve_xattrs: bool,

        /// strip filesystem timestamps when extracting tar-based archives
        #[arg(long)]
        strip_timestamps: bool,

        /// preserve ownership (uid/gid) when extracting tar-based archives
        #[arg(long)]
        preserve_ownership: bool,

        /// overwrite existing files
        #[arg(short = 'y', long)]
        overwrite: bool,

        #[command(flatten)]
        password: PasswordArgs,
    },

    /// list archive contents
    #[command(alias = "l")]
    List {
        /// archive file to list
        archive: PathBuf,
    },

    /// test archive integrity
    #[command(alias = "t")]
    Test {
        /// archive file to test
        archive: PathBuf,
    },
}

#[derive(Args)]
pub struct PasswordArgs {
    /// prompt securely for a password (zst and 7z; confirm when creating)
    #[arg(short = 'p', long, conflicts_with = "password_stdin")]
    password: bool,

    /// read one password line from stdin, for scripts
    #[arg(long, conflicts_with = "password")]
    password_stdin: bool,
}

impl PasswordArgs {
    pub fn uses_stdin(&self) -> bool {
        self.password_stdin
    }

    pub fn is_requested(&self) -> bool {
        self.password || self.password_stdin
    }

    pub fn resolve(&self, confirm: bool) -> crate::Result<Option<String>> {
        use anyhow::{bail, Context};
        use std::io::IsTerminal;

        if self.password {
            return crate::password::prompt(confirm).map(Some);
        }
        if self.password_stdin {
            if std::io::stdin().is_terminal() {
                bail!("--password-stdin requires redirected input; use -p for a hidden prompt");
            }
            return crate::password::read_line(&mut std::io::stdin().lock())
                .context("Failed to read password from stdin")
                .map(Some);
        }
        Ok(None)
    }
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
        input: &[PathBuf],
        output: Option<PathBuf>,
        format: Option<crate::formats::Format>,
    ) -> crate::Result<PathBuf> {
        if let Some(output) = output {
            return Ok(output);
        }

        if input.len() != 1 {
            return Err(anyhow::anyhow!(
                "multiple inputs require an explicit output path with -o/--output"
            ));
        }

        let mut path = input[0].to_path_buf();
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
        Ok(path)
    }

    /// get extraction directory, defaulting to current directory
    pub fn get_extract_dir(destination: Option<PathBuf>, directory: Option<PathBuf>) -> PathBuf {
        directory
            .or(destination)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[cfg(test)]
mod password_tests {
    use super::*;

    #[test]
    fn password_flag_does_not_consume_paths() {
        for flag in ["-p", "--password"] {
            let cli = Cli::try_parse_from(["zzz", "c", flag, "input.txt"]).unwrap();
            let Commands::Compress {
                input,
                password,
                kdf,
                ..
            } = cli.command
            else {
                panic!("expected compress command");
            };
            assert_eq!(input, [PathBuf::from("input.txt")]);
            assert!(password.is_requested());
            assert!(!password.uses_stdin());
            assert_eq!(kdf.unwrap_or_default(), crate::encryption::KdfPreset::Hi);

            let cli = Cli::try_parse_from(["zzz", "x", flag, "archive.zst", "dest"]).unwrap();
            let Commands::Extract {
                archive,
                destination,
                ..
            } = cli.command
            else {
                panic!("expected extract command");
            };
            assert_eq!(archive, PathBuf::from("archive.zst"));
            assert_eq!(destination, Some(PathBuf::from("dest")));
        }
    }

    #[test]
    fn password_sources_conflict_and_inline_values_are_rejected() {
        for command in ["c", "x"] {
            assert!(
                Cli::try_parse_from(["zzz", command, "file.zst", "-p", "--password-stdin",])
                    .is_err()
            );
            assert!(
                Cli::try_parse_from(["zzz", command, "file.zst", "--password=secret",]).is_err()
            );
        }
    }
}
