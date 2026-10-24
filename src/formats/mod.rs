use std::io::Write;

use crate::detect::Format;
use crate::error::{Error, Result};

#[cfg(feature = "gzip")]
pub mod gzip;
#[cfg(feature = "png")]
pub mod png;
#[cfg(feature = "zip")]
pub mod zip;

/// Options a handler may need. Everything here is explicit: there are no
/// hidden defaults inside handlers that change what is written.
#[derive(Clone, Copy, Debug)]
#[derive(Clone, Copy)]
pub struct Options {
    pub zopfli: bool,
    pub max_pixels: u64,
    /// Total uncompressed size a ZIP archive may expand to.
    pub max_uncompressed: u64,
    /// Maximum uncompressed-to-compressed ratio for a ZIP archive.
    pub max_ratio: u64,
    /// Optimize supported files stored inside archives.
    pub nested: bool,
    /// How deep nested processing may go.
    pub max_depth: u32,
    /// Current nesting depth; 0 for a file given on the command line.
    pub depth: u32,
}

/// What a handler produced.
#[derive(Clone, Copy, Debug)]
pub enum Cand {
    /// A candidate was written; the value is its length in bytes.
    Written(u64),
    /// No candidate was strictly smaller than the input.
    NoGain,
}

/// Runs the lossless transformation for `format`, writing the candidate to `dst`.
///
/// The handler never touches `src`. It must refuse anything it cannot fully
/// validate rather than produce a best-effort output.
pub fn optimize(
    format: Format,
    input: &[u8],
    dst: &mut dyn Write,
    opts: &Options,
) -> Result<Cand> {
    let cand = match format {
        #[cfg(feature = "png")]
        Format::Png => png::optimize(input, dst, opts)?,
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::optimize(input, dst, opts)?,
        #[cfg(feature = "zip")]
        Format::Zip => zip::optimize(input, dst, opts)?,
        #[cfg(feature = "png")]
        Format::Ico => png::optimize_icon(input, dst, opts)?,
        #[allow(unreachable_patterns)]
        other => {
            return Err(Error::internal(format!(
                "format `{}` is not compiled into this build",
                other.id()
            )))
        }
    };
    dst.flush()?;
    Ok(cand)
}

/// Checks a candidate against its contract, using decoders that are independent
/// of the code that wrote it.
pub fn verify(format: Format, original: &[u8], candidate: &[u8]) -> Result<()> {
    match format {
        #[cfg(feature = "png")]
        Format::Png => png::verify(original, candidate),
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::verify(original, candidate),
        #[cfg(feature = "zip")]
        Format::Zip => zip::verify(original, candidate),
        #[cfg(feature = "png")]
        Format::Ico => png::verify_icon(original, candidate),
        #[allow(unreachable_patterns)]
        other => Err(Error::internal(format!(
            "format `{}` is not compiled into this build",
            other.id()
        ))),
    }
}

/// Static description of what the build supports, for `tzim formats`.
// The pushes below are `#[cfg]`-gated, so the vector cannot be built as a literal.
#[allow(clippy::vec_init_then_push)]
pub fn supported() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rows = Vec::new();
    #[cfg(feature = "png")]
    rows.push((
        "PNG",
        "recompress IDAT, re-select scanline filters",
        "all non-IDAT chunks preserved byte for byte",
    ));
    #[cfg(feature = "gzip")]
    rows.push((
        "gzip",
        "recompress the DEFLATE body",
        "header preserved byte for byte (.gz, .tgz, .svgz)",
    ));
    #[cfg(feature = "png")]
    rows.push((
        "ICO/CUR",
        "optimize embedded PNG images",
        "directory, entry fields, order and non-PNG images",
    ));
    #[cfg(feature = "zip")]
    rows.push((
        "ZIP",
        "recompress DEFLATE entries",
        "names, extra fields, comments, order and attributes; signed archives refused",
    ));
    rows
}
