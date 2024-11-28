use clap::Parser;
use is_terminal::IsTerminal;
use pager::Pager;
use std::error::Error;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use tempfile::NamedTempFile;

mod optpipeline;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Display diffs of LLVM IR changes between optimization passes"
)]
#[command(after_help = "Example:
   # View optimization changes for function 'foo':
   clang input.c -O2 -mllvm -print-before-all -mllvm -print-after-all -mllvm -filter-print-funcs=foo -S -emit-llvm 2>&1 | optpipeline

   # From a saved dump file:
   optpipeline dump.txt")]
struct Args {
    /// Path to LLVM pass dump file. If not provided, reads from stdin
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,
}

fn read_input(args: &Args) -> Result<String, io::Error> {
    match &args.input {
        Some(path) => std::fs::read_to_string(path),
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let dump = read_input(&args).map_err(|e| format!("Failed to read input: {}", e))?;
    let result = optpipeline::process(&dump);

    let is_terminal = std::io::stdout().is_terminal();
    if is_terminal {
        Pager::with_default_pager("less -R").setup();
    }

    for (func, pipeline) in result.iter() {
        println!("Function: {}\n", func);
        for pass in pipeline {
            let mut old = NamedTempFile::new()?;
            write!(old, "{}", pass.before)?;
            let mut new = NamedTempFile::new()?;
            write!(new, "{}", pass.after)?;

            let status = Command::new("difft")
                .arg("--color")
                .arg(if is_terminal { "always" } else { "never" })
                .arg(&pass.name)
                .arg(old.path().to_str().unwrap())
                .arg("0000000000000000000000000000000000000000")
                .arg("100644")
                .arg(new.path().to_str().unwrap())
                .arg("0000000000000000000000000000000000000000")
                .arg("100644")
                .env("GIT_DIFF_PATH_TOTAL", "15")
                .env("GIT_DIFF_PATH_COUNTER", "13")
                .status()
                .map_err(|e| format!("Failed to execute difft: {}", e))?;

            if !status.success() {
                return Err("difft command failed".into());
            }
        }
    }

    Ok(())
}
