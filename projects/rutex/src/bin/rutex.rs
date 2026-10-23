use clap::Parser;
use rutex::render;
use std::path::PathBuf;

/// RuTeX CLI tool for rendering TeX formulas to SVG
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The TeX formula to render (e.g., "a^2 + b^2 = c^2")
    #[arg(short, long)]
    tex: String,

    /// Path to the font file (.ttf)
    #[arg(short, long)]
    font: String,

    /// Output SVG file path (prints to stdout if not provided)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match render(&cli.tex, &cli.font).await {
        Ok(svg) => {
            if let Some(output_path) = cli.output {
                std::fs::write(&output_path, svg)?;
                eprintln!("SVG rendered successfully to {:?}", output_path);
            } else {
                println!("{}", svg);
            }
        }
        Err(e) => {
            eprintln!("Error rendering TeX: {:?}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
