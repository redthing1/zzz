//! zzz - simple, fast compression tool for .zst archives

use clap::Parser;
use std::process;
use zzz::{
    cli::{Cli, Commands},
    compress, extract,
    filter::FileFilter,
    formats::{CompressionOptions, ExtractionOptions},
    list,
};

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> zzz::Result<()> {
    match cli.command {
        Commands::Compress {
            input,
            output,
            level,
            progress,
            exclude,
            no_default_excludes,
        } => {
            let output_path = Cli::get_output_path(&input, output);

            // check if output already exists and prompt user
            if output_path.exists() {
                let prompt_message = format!(
                    "output file '{}' already exists. overwrite?",
                    output_path.display()
                );
                if !zzz::utils::prompt_yes_no(&prompt_message) {
                    println!("operation cancelled");
                    return Ok(());
                }
            }

            let options = CompressionOptions {
                level,
                threads: cli.threads,
                ..Default::default()
            };

            let filter = FileFilter::new(!no_default_excludes, &exclude)?;

            let stats =
                compress::compress(&input, &output_path, options, filter, progress, cli.verbose)?;

            if !cli.verbose {
                println!(
                    "compressed {} ({}) -> {} ({})",
                    input.display(),
                    zzz::utils::format_bytes(stats.input_size),
                    output_path.display(),
                    zzz::utils::format_bytes(stats.output_size)
                );
            }
        }

        Commands::Extract {
            archive,
            destination,
            directory,
            overwrite,
        } => {
            let extract_dir = Cli::get_extract_dir(destination, directory);

            let options = ExtractionOptions {
                overwrite,
                ..Default::default()
            };

            extract::extract(&archive, &extract_dir, options, cli.verbose)?;
        }

        Commands::List { archive } => {
            list::list(&archive, cli.verbose)?;
        }
    }

    Ok(())
}
