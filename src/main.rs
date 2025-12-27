//! zzz - simple, fast compression multitool

use clap::Parser;
use std::process;
use zzz_arc::{
    cli::{Cli, Commands},
    compress, extract,
    filter::FileFilter,
    formats::{CompressionFormat, CompressionOptions, ExtractionOptions},
    list,
};

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> zzz_arc::Result<()> {
    match cli.command {
        Commands::Compress {
            input,
            output,
            level,
            progress,
            exclude,
            keep_xattrs,
            no_default_excludes,
            format,
            password,
        } => {
            let output_path = Cli::get_output_path(&input, output, format);

            // check if output already exists and prompt user
            if output_path.exists() {
                let prompt_message = format!(
                    "output file '{}' already exists. overwrite?",
                    output_path.display()
                );
                if !zzz_arc::utils::prompt_yes_no(&prompt_message) {
                    println!("operation cancelled");
                    return Ok(());
                }
            }

            let options = CompressionOptions {
                level,
                threads: cli.threads,
                password,
                strip_xattrs: !keep_xattrs,
                ..Default::default()
            };

            let filter = FileFilter::new(!no_default_excludes, &exclude)?;

            let stats = compress::compress(
                &input,
                &output_path,
                options,
                filter,
                progress,
                cli.verbose,
                format,
            )?;

            if !cli.verbose {
                println!(
                    "compressed {} ({}) -> {} ({})",
                    input.display(),
                    zzz_arc::utils::format_bytes(stats.input_size),
                    output_path.display(),
                    zzz_arc::utils::format_bytes(stats.output_size)
                );
            }
        }

        Commands::Extract {
            archive,
            destination,
            directory,
            strip_components,
            keep_xattrs,
            overwrite,
            password,
        } => {
            let extract_dir = Cli::get_extract_dir(destination, directory);

            let options = ExtractionOptions {
                overwrite,
                strip_components,
                strip_xattrs: !keep_xattrs,
                password,
            };

            extract::extract(&archive, &extract_dir, options, cli.verbose)?;
        }

        Commands::List { archive } => {
            list::list(&archive, cli.verbose)?;
        }

        Commands::Test { archive } => {
            // Detect format and test integrity
            let format = zzz_arc::formats::Format::detect(&archive)?;

            match format {
                zzz_arc::formats::Format::Zip => {
                    zzz_arc::formats::zip::ZipFormat::test_integrity(&archive)?
                }
                zzz_arc::formats::Format::SevenZ => {
                    zzz_arc::formats::sevenz::SevenZFormat::test_integrity(&archive)?
                }
                zzz_arc::formats::Format::Gzip => {
                    zzz_arc::formats::gz::GzipFormat::test_integrity(&archive)?
                }
                zzz_arc::formats::Format::Xz => {
                    zzz_arc::formats::xz::XzFormat::test_integrity(&archive)?
                }
                zzz_arc::formats::Format::Zstd => {
                    zzz_arc::formats::zstd::ZstdFormat::test_integrity(&archive)?
                }
                zzz_arc::formats::Format::Rar => {
                    zzz_arc::formats::rar::RarFormat::test_integrity(&archive)?
                }
            }

            println!("{} integrity: OK", archive.display());
        }
    }

    Ok(())
}
