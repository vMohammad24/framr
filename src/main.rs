use std::io::Cursor;
use std::path::PathBuf;

use clap::Parser;
use image::ImageFormat;
use libwayshot::region::{Position, Region};
use libwayshot::{LogicalRegion, WayshotConnection};
use slurp_rs::SelectOptions;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

#[derive(Parser)]
#[command(name = "framr")]
struct Cli {
    /// Screen to capture
    #[arg(short, long)]
    screen: Option<usize>,

    /// List available screens
    #[arg(long)]
    screens: bool,

    /// Select an area to capture
    #[arg(short, long)]
    area: bool,

    /// Copy screenshot to clipboard without saving
    #[arg(short, long)]
    copy: bool,

    /// Output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Filename (e.g. "screenshot_%Y-%m-%d.png")
    #[arg(long)]
    filename: Option<String>,
}

fn capture(cli: &Cli) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let conn = WayshotConnection::new()?;
    let image = if cli.area {
        let selection = slurp_rs::select_region(SelectOptions::default())?;
        let rect = &selection.rect;
        conn.screenshot(
            LogicalRegion {
                inner: Region {
                    position: Position {
                        x: rect.x,
                        y: rect.y,
                    },
                    size: libwayshot::Size {
                        width: rect.width as u32,
                        height: rect.height as u32,
                    },
                },
            },
            true,
        )?
    } else if let Some(screen_num) = cli.screen {
        let outputs = conn.get_all_outputs();
        let output = outputs.get(screen_num).ok_or_else(|| {
            format!(
                "screen {}: only {} screens available",
                screen_num,
                outputs.len()
            )
        })?;
        conn.screenshot_single_output(output, true)?
    } else {
        conn.screenshot_all(true)?
    };

    let mut buf = Cursor::new(Vec::new());
    image.write_to(&mut buf, ImageFormat::Png)?;
    Ok(buf.into_inner())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.screens {
        let conn = WayshotConnection::new()?;
        for (i, output) in conn.get_all_outputs().iter().enumerate() {
            let pos = output.logical_position();
            let size = output.logical_size();
            println!(
                "{}: {} ({}x{}+{}+{})",
                i, output.name, size.width, size.height, pos.x, pos.y
            );
        }
        return Ok(());
    }

    let png_bytes = capture(&cli)?;

    if cli.copy {
        match unsafe { libc::fork() } {
            -1 => return Err("fork failed".into()),
            0 => {
                let mut clipboard_opts = ClipboardOptions::new();
                clipboard_opts.foreground(true).seat(Seat::All);
                let _ = clipboard_opts.copy(
                    Source::Bytes(png_bytes.into()),
                    MimeType::Specific("image/png".into()),
                );
                std::process::exit(0);
            }
            _ => return Ok(()),
        }
    }

    let filename = chrono::Local::now()
        .format(
            &cli.filename
                .unwrap_or_else(|| "screenshot_%Y-%m-%d_%H-%M-%S.png".to_string()),
        )
        .to_string();

    let path = match &cli.output {
        Some(dir) => dir.join(&filename),
        None => PathBuf::from(&filename),
    };

    std::fs::write(&path, &png_bytes)?;

    Ok(())
}
