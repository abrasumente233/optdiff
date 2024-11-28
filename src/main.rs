use pager::Pager;
use std::error::Error;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

mod optpipeline;

fn main() -> Result<(), Box<dyn Error>> {
    Pager::with_default_pager("less -R").setup();
    let dump = std::fs::read_to_string("dumpfuck.txt").unwrap();
    let result = optpipeline::process(&dump);
    //println!("{:#?}", result);

    let mut before = NamedTempFile::new()?;
    writeln!(before, "hello")?;

    let mut after = NamedTempFile::new()?;
    writeln!(after, "hello world")?;

    for pass in &result["a"] {
        let mut old = NamedTempFile::new()?;
        write!(old, "{}", pass.before)?;

        let mut new = NamedTempFile::new()?;
        write!(new, "{}", pass.after)?;

        // Reference: https://git-scm.com/docs/git#Documentation/git.txt-codeGITEXTERNALDIFFcode
        Command::new("difft")
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
            .expect("failed to execute process");
    }

    // Call diff with environment variable PAGER set to "less"

    //println!("{}", String::from_utf8_lossy(&diff.stdout));
    Ok(())
}
