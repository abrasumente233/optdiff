//! Taken from https://github.com/gimli-rs/cpp_demangle/blob/master/examples/cppfilt.rs
#![allow(unused)]

use cpp_demangle::{BorrowedSymbol, DemangleOptions};
use std::io::{self, BufRead, Write};

/// Find the index of the first (potential) occurrence of a mangled C++ symbol
/// in the given `haystack`.
fn find_mangled(haystack: &[u8]) -> Option<usize> {
    if haystack.is_empty() {
        return None;
    }

    for i in 0..haystack.len() - 1 {
        if haystack[i] == b'_' {
            match (
                haystack[i + 1],
                haystack.get(i + 2),
                haystack.get(i + 3),
                haystack.get(i + 4),
            ) {
                (b'Z', _, _, _) | (b'_', Some(b'Z'), _, _) | (b'_', Some(b'_'), Some(b'Z'), _) => {
                    return Some(i)
                }
                (b'_', Some(b'_'), Some(b'_'), Some(b'Z')) => return Some(i),
                _ => (),
            }
        }
    }

    None
}

/// Print the given `line` to `out`, with all mangled C++ symbols replaced with
/// their demangled form.
pub fn demangle_line<W>(out: &mut W, line: &[u8], options: DemangleOptions) -> io::Result<()>
where
    W: Write,
{
    let mut line = line;

    while let Some(idx) = find_mangled(line) {
        write!(out, "{}", String::from_utf8_lossy(&line[..idx]))?;

        let prefix_len = if idx + 1 < line.len() {
            match (
                line[idx + 1],
                line.get(idx + 2),
                line.get(idx + 3),
                line.get(idx + 4),
            ) {
                (b'Z', _, _, _) => 2,                            // _Z
                (b'_', Some(b'Z'), _, _) => 3,                   // __Z
                (b'_', Some(b'_'), Some(b'Z'), _) => 4,          // ___Z
                (b'_', Some(b'_'), Some(b'_'), Some(b'Z')) => 5, // ____Z
                _ => 2, // fallback case, shouldn't happen due to find_mangled logic
            }
        } else {
            2 // fallback case for end of input
        };

        if let Ok((sym, tail)) = BorrowedSymbol::with_tail(&line[idx..]) {
            let demangled = sym
                .demangle(&options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            write!(out, "{}", demangled)?;
            line = tail;
        } else {
            write!(
                out,
                "{}",
                String::from_utf8_lossy(&line[idx..idx + prefix_len])
            )?;
            line = &line[idx + prefix_len..];
        }
    }

    write!(out, "{}", String::from_utf8_lossy(line))
}

/// Print all the lines from the given `input` to `out`, with all mangled C++
/// symbols replaced with their demangled form.
pub fn demangle_all<R, W>(input: &mut R, out: &mut W, options: DemangleOptions) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let mut buf = vec![];

    while input.read_until(b'\n', &mut buf)? > 0 {
        let nl = buf.ends_with(b"\n");
        if nl {
            buf.pop();
        }
        demangle_line(out, &buf[..], options)?;
        if nl {
            writeln!(out)?;
        }
        buf.clear();
    }

    Ok(())
}

pub struct DemangleBuilder {
    options: DemangleOptions,
}

impl Default for DemangleBuilder {
    fn default() -> Self {
        Self {
            options: DemangleOptions::new(),
        }
    }
}

impl DemangleBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn no_params(mut self) -> Self {
        self.options = self.options.no_params();
        self
    }

    pub fn no_return_type(mut self) -> Self {
        self.options = self.options.no_return_type();
        self
    }

    pub fn hide_expression_literal_types(mut self) -> Self {
        self.options = self.options.hide_expression_literal_types();
        self
    }

    pub fn build(self) -> DemangleOptions {
        self.options
    }
}
