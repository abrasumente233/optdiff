use clap::Parser;
use is_terminal::IsTerminal;
use optpipeline::Pass;
use similar::TextDiff;
use std::error::Error;
use std::io::{self, Read};
use std::path::PathBuf;

#[cfg(unix)]
use pager::Pager;

mod optpipeline;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Display diffs of LLVM IR changes between optimization passes"
)]
#[command(after_help = "Example:
   # View optimization changes for function 'foo':
   clang input.c -O2 -mllvm -print-before-all -mllvm -print-after-all -mllvm -filter-print-funcs=foo -c -o /dev/null 2>&1 | optdiff

   # `-mllvm -filter-print-funcs=foo` is optional and if left out, optdiff will print diffs for all functions in the dump.

   # From a saved dump file:
   optdiff dump.txt")]
struct Args {
    /// Path to LLVM pass dump file. If not provided, reads from stdin
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,

    /// Hide optimization passes that don't modify the IR
    #[arg(short = 's', long = "skip-unchanged")]
    skip_unchanged: bool,

    /// Only show passes for specified function
    #[arg(short = 'f', long = "function")]
    function: Option<String>,

    /// List available functions
    #[arg(short = 'l', long = "list")]
    list: bool,
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

fn print_func(
    func_name: &str,
    pipeline: &[Pass],
    skip_unchanged: bool,
) -> Result<(), Box<dyn Error>> {
    println!("Function: {}\n", func_name);
    for pass in pipeline {
        if skip_unchanged && pass.before == pass.after {
            continue;
        }

        let before = pass.before.clone() + "\n";
        let after = pass.after.clone() + "\n";
        let diff = TextDiff::from_lines(&before, &after);

        println!("diff --git a/{} b/{}", &pass.name, &pass.name);
        println!("--- a/{}", &pass.name);
        println!("+++ b/{}", &pass.name);
        println!("{}", diff.unified_diff().context_radius(10));
    }

    Ok(())
}

fn enter_pager() {
    #[cfg(unix)]
    if std::io::stdout().is_terminal() {
        Pager::with_default_pager("less -R").setup();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let dump = read_input(&args).map_err(|e| format!("Failed to read input: {}", e))?;
    let result = optpipeline::process(&dump);

    if args.list {
        for func in result.keys() {
            println!("{func}");
        }
        return Ok(());
    }

    if let Some(expected) = args.function {
        let (func_name, pipeline) = result
            .iter()
            .find(|(func_name, _)| *func_name == &expected)
            .ok_or_else(|| format!("Function '{}' was not found in the input", expected))?;
        enter_pager();
        print_func(func_name, pipeline, args.skip_unchanged)?;
    } else {
        enter_pager();
        for (func, pipeline) in result.iter() {
            print_func(func, pipeline, args.skip_unchanged)?;
        }
    }

    Ok(())
}
