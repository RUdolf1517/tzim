//! Small, testable yes/no interaction for the one confirmation-only path.

use std::io::{self, BufRead, Write};
use std::path::Path;

/// Ask about the visible FITS IMAGE-to-BINTABLE transformation. Only an
/// explicit `y` or `yes` (case-insensitive) consents; EOF, invalid input, and
/// input/output errors all mean No.
pub fn ask_fits_tile<R: BufRead, W: Write>(path: &Path, input: &mut R, output: &mut W) -> bool {
    if writeln!(
        output,
        "tzim: --fits-tile changes an eligible FITS IMAGE HDU into a tile-compressed BINTABLE. Rewrite {}? [y/N]",
        path.display()
    )
    .and_then(|()| output.flush())
    .is_err()
    {
        return false;
    }

    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    struct BrokenInput;

    impl Read for BrokenInput {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed input"))
        }
    }

    impl BufRead for BrokenInput {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed input"))
        }

        fn consume(&mut self, _amount: usize) {}
    }

    struct BrokenOutput;

    impl Write for BrokenOutput {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
        }
    }

    #[test]
    fn only_explicit_yes_confirms() {
        for answer in [b"y\n".as_slice(), b"Y\n", b"yes\n", b" YES \n"] {
            assert!(ask_fits_tile(
                Path::new("image.fits"),
                &mut Cursor::new(answer),
                &mut Vec::new()
            ));
        }
        for answer in [b"\n".as_slice(), b"n\n", b"no\n", b"sure\n"] {
            assert!(!ask_fits_tile(
                Path::new("image.fits"),
                &mut Cursor::new(answer),
                &mut Vec::new()
            ));
        }
    }

    #[test]
    fn eof_and_closed_stdin_are_no() {
        assert!(!ask_fits_tile(
            Path::new("image.fits"),
            &mut Cursor::new(b""),
            &mut Vec::new()
        ));
        assert!(!ask_fits_tile(
            Path::new("image.fits"),
            &mut BrokenInput,
            &mut Vec::new()
        ));
    }

    #[test]
    fn output_failure_is_no() {
        assert!(!ask_fits_tile(
            Path::new("image.fits"),
            &mut Cursor::new(b"y\n"),
            &mut BrokenOutput
        ));
    }

    #[test]
    fn the_prompt_names_the_file_and_exact_transformation() {
        let mut output = Vec::new();
        assert!(!ask_fits_tile(
            Path::new("survey/image.fits"),
            &mut Cursor::new(b"n\n"),
            &mut output
        ));
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("survey/image.fits"));
        assert!(text.contains("IMAGE HDU into a tile-compressed BINTABLE"));
        assert!(text.ends_with("[y/N]\n"));
    }
}
