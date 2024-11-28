use pager::Pager;
use std::error::Error;
use std::io::{self, Read, Write};
use std::process::Command;
use tempfile::NamedTempFile;
mod optpipeline;

fn read_input() -> Result<String, io::Error> {
    let mut args = std::env::args().skip(1);
    match args.next() {
        Some(path) => std::fs::read_to_string(path),
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let dump = read_input().map_err(|e| format!("Failed to read input: {}", e))?;
    let result = optpipeline::process(&dump);

    Pager::with_default_pager("less -R").setup();

    let mut before = NamedTempFile::new()?;
    writeln!(before, "hello")?;
    let mut after = NamedTempFile::new()?;
    writeln!(after, "hello world")?;

    for pass in &result["a"] {
        let mut old = NamedTempFile::new()?;
        write!(old, "{}", pass.before)?;
        let mut new = NamedTempFile::new()?;
        write!(new, "{}", pass.after)?;

        let status = Command::new("difft")
            .arg("--color")
            .arg("always")
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

    Ok(())
}

