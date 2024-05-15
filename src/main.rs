use std::error::Error;

mod optpipeline;

fn main() -> Result<(), Box<dyn Error>> {
    let dump = std::fs::read_to_string("dump.txt").unwrap();
    let result = optpipeline::process(&dump);
    println!("{:#?}", result);
    Ok(())
}
