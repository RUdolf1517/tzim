use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::detect::Format;
use crate::error::{Error, Result};

#[cfg(feature = "gzip")]
pub mod gzip;
#[cfg(feature = "png")]
pub mod png;

/// Options a handler may need. Everything here is explicit: there are no
/// hidden defaults inside handlers that change what is written.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub zopfli: bool,
    pub max_pixels: u64,
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
pub fn optimize(format: Format, src: &Path, dst: &mut File, opts: &Options) -> Result<Cand> {
    let cand = match format {
        #[cfg(feature = "png")]
        Format::Png => png::optimize(src, dst, opts)?,
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::optimize(src, dst, opts)?,
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
pub fn verify(format: Format, original: &Path, candidate: &Path) -> Result<()> {
    match format {
        #[cfg(feature = "png")]
        Format::Png => png::verify(original, candidate),
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::verify(original, candidate),
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
    rows
}
