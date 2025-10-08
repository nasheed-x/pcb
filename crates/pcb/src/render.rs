use anyhow::Result;
use clap::Args;
use pcb_layout::LayoutError;
use pcb_render::{process_for_render, render_pcb3d};
use pcb_ui::prelude::*;
use std::path::PathBuf;

use crate::build::{build, create_diagnostics_passes};
use crate::file_walker;

/// Arguments for the `render` command
#[derive(Args, Debug, Default, Clone)]
#[command(about = "Generate PCB3D files from .zen files for 3D visualization")]
pub struct RenderArgs {
    /// Output .pcb3d file path
    #[arg(short = 'o', long, value_name = "OUTPUT")]
    pub output: Option<PathBuf>,

    /// Skip opening the output file after generation
    #[arg(long)]
    pub no_open: bool,

    /// One or more .zen files to process for rendering.
    /// When omitted, all .zen files in the current directory tree are processed.
    #[arg(value_name = "PATHS", value_hint = clap::ValueHint::AnyPath)]
    pub paths: Vec<PathBuf>,

    /// Disable network access (offline mode) - only use vendored dependencies
    #[arg(long = "offline")]
    pub offline: bool,
}

pub fn execute(args: RenderArgs) -> Result<()> {
    // Collect .zen files to process
    let zen_paths = file_walker::collect_zen_files(&args.paths, false)?;

    if zen_paths.is_empty() {
        let cwd = std::env::current_dir()?;
        anyhow::bail!(
            "No .zen source files found in {}",
            cwd.canonicalize().unwrap_or(cwd).display()
        );
    }

    // Check if multiple files with custom output
    if args.output.is_some() && zen_paths.len() > 1 {
        anyhow::bail!("Cannot specify --output with multiple input files");
    }

    let mut has_errors = false;
    let mut rendered_count = 0;

    // Process each .zen file
    for zen_path in zen_paths {
        let file_name = zen_path.file_name().unwrap().to_string_lossy();

        // Build the schematic
        let Some(schematic) = build(
            &zen_path,
            args.offline,
            create_diagnostics_passes(&[]),
            &mut has_errors,
        ) else {
            continue;
        };

        // Get the PCB file from layout
        let pcb_file = match process_for_render(&schematic, &zen_path) {
            Ok(pcb_file) => pcb_file,
            Err(LayoutError::NoLayoutPath) => {
                // Skip files without layout
                println!(
                    "{} {} (no layout)",
                    pcb_ui::icons::warning(),
                    file_name.with_style(Style::Yellow).bold()
                );
                continue;
            }
            Err(e) => {
                println!(
                    "{} {}: {}",
                    pcb_ui::icons::error(),
                    file_name.with_style(Style::Red).bold(),
                    e
                );
                has_errors = true;
                continue;
            }
        };

        // Determine output path
        let output_path = if let Some(ref output) = args.output {
            output.clone()
        } else {
            // Default to same directory as source with .pcb3d extension
            zen_path.with_extension("pcb3d")
        };

        // Render the PCB to PCB3D
        match render_pcb3d(&pcb_file, &output_path) {
            Ok(_) => {
                rendered_count += 1;
                let output_name = output_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_else(|| output_path.to_string_lossy());
                
                println!(
                    "{} {} -> {}",
                    pcb_ui::icons::success(),
                    file_name.with_style(Style::Green).bold(),
                    output_name
                );

                // Open the file unless --no-open was specified
                if !args.no_open {
                    if let Err(e) = open::that(&output_path) {
                        eprintln!("Failed to open {}: {}", output_path.display(), e);
                    }
                }
            }
            Err(e) => {
                println!(
                    "{} {}: {}",
                    pcb_ui::icons::error(),
                    file_name.with_style(Style::Red).bold(),
                    e
                );
                has_errors = true;
            }
        }
    }

    if has_errors {
        anyhow::bail!("Rendering failed with errors");
    }

    if rendered_count == 0 {
        println!("No files with layouts found to render.");
    } else {
        println!(
            "\n{} Successfully rendered {} file{}.",
            pcb_ui::icons::success(),
            rendered_count,
            if rendered_count == 1 { "" } else { "s" }
        );
    }

    Ok(())
}