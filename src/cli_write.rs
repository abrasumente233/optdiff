// https://users.rust-lang.org/t/broken-pipe-when-attempt-to-write-to-stdout/111186/6

#[macro_export]
macro_rules! cli_write {
    ($writer:expr, $($arg:tt)*) => {
        write!($writer, $($arg)*).map_err(|e| match e.kind() {
            std::io::ErrorKind::BrokenPipe => {
                std::process::exit(1);
            }
            _ => e,
        })
    }
}

#[macro_export]
macro_rules! cli_writeln {
    ($writer:expr, $($arg:tt)*) => {
        writeln!($writer, $($arg)*).map_err(|e| match e.kind() {
            std::io::ErrorKind::BrokenPipe => {
                std::process::exit(1);
            }
            _ => e,
        })
    }
}
