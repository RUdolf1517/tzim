use std::io::Write;

use crate::detect::Format;
use crate::error::{Error, Result};

#[cfg(feature = "fits")]
pub mod fits;
#[cfg(feature = "gzip")]
pub mod gzip;
#[cfg(feature = "jpeg")]
pub mod jpeg;
#[cfg(feature = "png")]
pub mod png;
#[cfg(feature = "tiff")]
pub mod tiff;
#[cfg(feature = "zip")]
pub mod zip;
#[cfg(feature = "zlib")]
pub mod zlib;

/// Whether this build contains a handler for `format`.
pub const fn compiled(format: Format) -> bool {
    match format {
        Format::Png => cfg!(feature = "png"),
        Format::Gzip => cfg!(feature = "gzip"),
        Format::Zip => cfg!(feature = "zip"),
        Format::Ico => cfg!(feature = "ico"),
        Format::Jpeg => cfg!(feature = "jpeg"),
        Format::Tiff => cfg!(feature = "tiff"),
        Format::Zlib => cfg!(feature = "zlib"),
        Format::Fits => cfg!(feature = "fits"),
    }
}

/// The formats this build can handle, in the order `--list-formats` prints them.
pub fn compiled_names() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "png") {
        out.push("png");
    }
    if cfg!(feature = "gzip") {
        out.push("gzip");
    }
    if cfg!(feature = "zip") {
        out.push("zip");
    }
    if cfg!(feature = "ico") {
        out.push("ico");
    }
    if cfg!(feature = "jpeg") {
        out.push("jpeg");
    }
    if cfg!(feature = "tiff") {
        out.push("tiff");
    }
    if cfg!(feature = "zlib") {
        out.push("zlib");
    }
    if cfg!(feature = "fits") {
        out.push("fits");
    }
    out
}

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
    pub tiff: bool,
    pub zlib: bool,
    pub fits: bool,
}

impl Default for Allow {
    /// Everything this build can do, except JPEG: it is opt-in because its gain
    /// is small and its code path is the most intricate in the project.
    fn default() -> Self {
        Allow {
            png: cfg!(feature = "png"),
            gzip: cfg!(feature = "gzip"),
            zip: cfg!(feature = "zip"),
            ico: cfg!(feature = "ico"),
            jpeg: false,
            tiff: cfg!(feature = "tiff"),
            zlib: cfg!(feature = "zlib"),
            // The handler changes an `IMAGE` HDU into a `BINTABLE`, which is a
            // visible change to a reader, so it does nothing unless the run asks
            // for it with `--fits-tile` (`Options::fits_tile`). Being compiled
            // in is not the same as being on.
            fits: cfg!(feature = "fits"),
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
            tiff: false,
            zlib: false,
            fits: false,
        };
        for name in names {
            let (format, slot) = match name.as_str() {
                "png" | "apng" => (Format::Png, &mut allow.png),
                "gzip" | "gz" => (Format::Gzip, &mut allow.gzip),
                "zip" => (Format::Zip, &mut allow.zip),
                "ico" | "cur" => (Format::Ico, &mut allow.ico),
                "jpeg" | "jpg" => (Format::Jpeg, &mut allow.jpeg),
                "tiff" | "tif" => (Format::Tiff, &mut allow.tiff),
                "zlib" => (Format::Zlib, &mut allow.zlib),
                "fits" => (Format::Fits, &mut allow.fits),
                other => {
                    return Err(Error::usage(format!(
                        "unknown format `{other}`; expected png, gzip, zip, ico, jpeg, tiff, \
                         zlib or fits"
                    )))
                }
            };
            if !compiled(format) {
                let built = compiled_names();
                return Err(Error::usage(format!(
                    "this build has no {} handler (compiled: {})",
                    format.id(),
                    if built.is_empty() {
                        "none".to_string()
                    } else {
                        built.join(", ")
                    }
                )));
            }
            *slot = true;
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
            Format::Tiff => self.tiff,
            Format::Zlib => self.zlib,
            Format::Fits => self.fits,
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
        if self.tiff {
            out.push("tiff");
        }
        if self.zlib {
            out.push("zlib");
        }
        if self.fits {
            out.push("fits");
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
    /// Remove metadata that does not affect the content, on request.
    ///
    /// This is the only option that makes the output differ from the input in
    /// something other than its encoding. It is off unless the user asks, and
    /// what it removes is a fixed list per format, kept next to the handler.
    pub strip_metadata: bool,
    /// Optimize supported files stored inside archives.
    pub nested: bool,
    /// How deep nested processing may go.
    pub max_depth: u32,
    /// Current nesting depth; 0 for a file given on the command line.
    pub depth: u32,
    /// Formats this run may process; nested children obey the same set.
    pub allow: Allow,
    /// Turn an uncompressed image HDU into a tile-compressed table (`--fits-tile`).
    ///
    /// Off unless the user asks, because the file stops being an `IMAGE` HDU and
    /// becomes a `BINTABLE`. The same shape as `strip_metadata`: one flag, one
    /// documented change, never implied by another.
    pub fits_tile: bool,
}

/// Whether a candidate may replace its original.
///
/// This is invariant I2 from `docs/guarantees.md` in one place. Without
/// `--strip-metadata` a candidate must be **strictly** smaller; with the flag an
/// **equal-size** candidate is legitimate, because zeroing a DOS timestamp or a
/// gzip MTIME removes metadata without removing bytes. A larger candidate is
/// never accepted, under any flag.
///
/// Written once because it was written three times, with three answers: the
/// pipeline rejected an equal candidate as an internal error while gzip, ZIP and
/// the `strip` fuzz target accepted it, which is issue #26 of the audit.
pub fn size_ok(original: usize, candidate: usize, strip: bool) -> bool {
    candidate > 0 && (candidate < original || (strip && candidate == original))
}

/// What a handler produced.
#[derive(Clone, Copy, Debug)]
pub enum Cand {
    /// A candidate was written; the value is its length in bytes.
    Written(u64),
    /// No candidate was smaller than the input (or, under `--strip-metadata`,
    /// no smaller and nothing to remove at the same size).
    NoGain,
}

/// Runs the lossless transformation for `format`, writing the candidate to `dst`.
///
/// The handler never touches `src`. It must refuse anything it cannot fully
/// validate rather than produce a best-effort output.
pub fn optimize(format: Format, input: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let cand = match format {
        #[cfg(feature = "png")]
        Format::Png => png::optimize(input, dst, opts)?,
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::optimize(input, dst, opts)?,
        #[cfg(feature = "zip")]
        Format::Zip => zip::optimize(input, dst, opts)?,
        #[cfg(feature = "ico")]
        Format::Ico => png::optimize_icon(input, dst, opts)?,
        #[cfg(feature = "jpeg")]
        Format::Jpeg => jpeg::optimize(input, dst, opts)?,
        #[cfg(feature = "tiff")]
        Format::Tiff => tiff::optimize(input, dst, opts)?,
        #[cfg(feature = "zlib")]
        Format::Zlib => zlib::optimize(input, dst, opts)?,
        #[cfg(feature = "fits")]
        Format::Fits => fits::optimize(input, dst, opts)?,
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

/// Checks a candidate against its contract.
///
/// How independent the checker is differs per format, and `docs/guarantees.md`
/// says which is which: PNG and gzip are decoded by crates written by someone
/// else, while ZIP and JPEG are compared by parsers from this repository. In no
/// case does verification reuse the encoder that produced the candidate.
pub fn verify(format: Format, original: &[u8], candidate: &[u8], opts: &Options) -> Result<()> {
    // G3 clause 4, checked here rather than left implied by the pipeline: a
    // verifier that accepts a larger candidate is not verifying the contract.
    if !size_ok(original.len(), candidate.len(), opts.strip_metadata) {
        return Err(Error::verify(format!(
            "candidate is {} bytes and the original {}; {}",
            candidate.len(),
            original.len(),
            if opts.strip_metadata {
                "a stripping rewrite may keep the size but never grow it"
            } else {
                "a rewrite must be strictly smaller"
            }
        )));
    }
    match format {
        #[cfg(feature = "png")]
        Format::Png => png::verify(original, candidate, opts),
        #[cfg(feature = "gzip")]
        Format::Gzip => gzip::verify(original, candidate, opts),
        #[cfg(feature = "zip")]
        Format::Zip => zip::verify(original, candidate, opts),
        #[cfg(feature = "ico")]
        Format::Ico => png::verify_icon(original, candidate, opts),
        #[cfg(feature = "jpeg")]
        Format::Jpeg => jpeg::verify(original, candidate, opts),
        #[cfg(feature = "tiff")]
        Format::Tiff => tiff::verify(original, candidate, opts),
        #[cfg(feature = "zlib")]
        Format::Zlib => zlib::verify(original, candidate, opts),
        #[cfg(feature = "fits")]
        Format::Fits => fits::verify(original, candidate, opts),
        #[allow(unreachable_patterns)]
        other => Err(Error::internal(format!(
            "format `{}` is not compiled into this build",
            other.id()
        ))),
    }
}

/// Static description of what the build supports, for `tzim --list-formats`.
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
    #[cfg(feature = "ico")]
    rows.push((
        "ICO/CUR",
        "optimize embedded PNG images",
        "directory, entry fields, order and non-PNG images",
    ));
    #[cfg(feature = "jpeg")]
    rows.push((
        "JPEG",
        "re-encode the entropy data with optimal Huffman tables",
        "every marker and every DCT coefficient preserved; opt-in via --formats",
    ));
    #[cfg(feature = "tiff")]
    rows.push((
        "TIFF",
        "recompress strips, or compress uncompressed ones",
        "every tag, entry order and geometry; strips decode identically",
    ));
    #[cfg(feature = "zip")]
    rows.push((
        "ZIP",
        "recompress DEFLATE entries",
        "names, extra fields, comments, order and attributes; signed archives refused",
    ));
    #[cfg(feature = "zlib")]
    rows.push((
        "ZLIB",
        "recompress the DEFLATE stream",
        "header preserved byte for byte; Adler-32 recomputed and matched",
    ));
    #[cfg(feature = "fits")]
    rows.push((
        "FITS",
        "tile-compress image HDUs (--fits-tile)",
        "the array round-trips byte for byte; every other HDU copied verbatim",
    ));
    rows
}

#[cfg(test)]
mod tests {
    use super::size_ok;

    /// The four combinations of invariant I2, in one place. `size_ok` is the only
    /// copy of the rule — the pipeline, the verifier dispatcher and the fuzzer all
    /// call it — so this test is the whole specification.
    #[test]
    fn equal_size_is_accepted_only_when_stripping() {
        assert!(size_ok(100, 99, false), "smaller is the normal case");
        assert!(
            !size_ok(100, 100, false),
            "without --strip-metadata, equal is not a gain"
        );
        assert!(
            size_ok(100, 99, true),
            "smaller is still accepted with the flag"
        );
        assert!(
            size_ok(100, 100, true),
            "zeroing a timestamp removes metadata without removing bytes"
        );
    }

    #[test]
    fn a_larger_candidate_is_never_accepted() {
        assert!(!size_ok(100, 101, false));
        assert!(!size_ok(100, 101, true), "no flag permits growth");
    }

    #[test]
    fn an_empty_candidate_is_never_accepted() {
        // A handler that wrote nothing has failed, not won: `0 < 100` would
        // otherwise read as an infinite gain.
        assert!(!size_ok(100, 0, false));
        assert!(!size_ok(100, 0, true));
        assert!(!size_ok(0, 0, true), "an empty original cannot be improved");
    }
}
