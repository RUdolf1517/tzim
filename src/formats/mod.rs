use std::io::Write;

use crate::detect::Format;
use crate::error::{Error, Result};

#[cfg(feature = "gzip")]
pub mod gzip;
pub mod jpeg;
#[cfg(feature = "png")]
pub mod png;
#[cfg(feature = "zip")]
pub mod zip;

/// Which formats a run is allowed to process.
///
/// Only JPEG is off by default: it is opt-in because its gain is small and its
/// code path is the most intricate in the project. `--formats` turns it on and
/// can also narrow the set for everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allow {
    pub png: bool,
    pub gzip: bool,
    pub zip: bool,
    pub ico: bool,
    pub jpeg: bool,
}

impl Default for Allow {
    fn default() -> Self {
        Allow {
            png: true,
            gzip: true,
            zip: true,
            ico: true,
            jpeg: false,
        }
    }
}

impl Allow {
    /// Builds the set named on the command line. An empty list is an error at
    /// the call site, not here: `--formats ""` names nothing and disables
    /// everything, which is a legitimate (if useless) request.
    pub fn named(names: &[String]) -> Result<Allow> {
        let mut allow = Allow {
            png: false,
            gzip: false,
            zip: false,
            ico: false,
            jpeg: false,
        };
        for name in names {
            match name.as_str() {
                "png" | "apng" => allow.png = true,
                "gzip" | "gz" => allow.gzip = true,
                "zip" => allow.zip = true,
                "ico" | "cur" => allow.ico = true,
                "jpeg" | "jpg" => allow.jpeg = true,
                other => {
                    return Err(Error::usage(format!(
                        "unknown format `{other}`; expected png, gzip, zip, ico or jpeg"
                    )))
                }
            }
        }
        Ok(allow)
    }

    pub fn permits(self, format: Format) -> bool {
        match format {
            Format::Png => self.png,
            Format::Gzip => self.gzip,
            Format::Zip => self.zip,
            Format::Ico => self.ico,
            Format::Jpeg => self.jpeg,
        }
    }

    /// Human-readable list of the enabled formats, for messages.
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.png {
            out.push("png");
        }
        if self.gzip {
            out.push("gzip");
        }
        if self.zip {
            out.push("zip");
        }
        if self.ico {
            out.push("ico");
        }
        if self.jpeg {
            out.push("jpeg");
        }
        out
    }
}

/// Options a handler may need. Everything here is explicit: there are no
/// hidden defaults inside handlers that change what is written.
#[derive(Clone, Copy, Debug)]
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
    /// Formats this run may process; nested children obey the same set.
    pub allow: Allow,
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
        Format::Jpeg => jpeg::optimize(input, dst, opts)?,
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
        Format::Jpeg => jpeg::verify(original, candidate),
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
    rows.push((
        "JPEG",
        "re-encode the entropy data with optimal Huffman tables",
        "every marker and every DCT coefficient preserved; opt-in via --formats",
    ));
    #[cfg(feature = "zip")]
    rows.push((
        "ZIP",
        "recompress DEFLATE entries",
        "names, extra fields, comments, order and attributes; signed archives refused",
    ));
    rows
}
