# FLAC: the audit behind the contract

Related: [guarantees §4.9](guarantees.md#49-flac--frame-re-encoding-79-non-default-opt-in) · [formats](formats.md) ·
[dependencies](dependencies.md) · [roadmap](roadmap.md)

This is stage 1 for FLAC — the audit that a contract is allowed to follow. It exists because the format
queue in the roadmap says "contract before code", and a contract written without the audit would be a
wish list: FLAC is the one format in the queue where the *lossless* part is easy and the *verifiable* part
is the whole problem.

**Historical status: refused after measurement (§7).** That refusal and its historical gain/cost figures remain
unchanged evidence; the current owner separately authorized the non-default #79 implementation. No historical
threshold or result in §7 has been rewritten.

**Current owner scope and exact-SHA evidence.** #79 is implemented behind the non-default `flac` feature. The
latest fixed corpus run at exact SHA `5967e318951f9c3945f0c256c3095b347483e234` retained all 43 frozen rows:
30 valid inputs (23 optimized, 7 already optimal), 7 documented refusals, and 6 invalid controls. All 23
optimized candidates passed external `flac --test` and `metaflac`, independent PCM comparison, and
STREAMINFO-MD5 checks. The full 43-row corpus totaled 109,472,390 → 108,727,820 bytes; the 23 optimized inputs
totaled 91,284,082 → 90,539,512 bytes; both views save 744,570 bytes. First-pass time was 6.371 seconds total
(102.685 ms/file median); the `flac`-only feature build was 836,384 bytes. Exact rows, manifest and commands are
in the [run report](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521735772) and artifact
`flac-corpus-5967e318951f9c3945f0c256c3095b347483e234` (no FLAC bytes). The earlier run remains historical.
This is measured sample evidence, not full-format admission or release approval.

---

## 1. What the file is

A FLAC stream is four ASCII bytes `fLaC`, a sequence of metadata blocks, and then frames to the end of the
file. Everything below is what the contract has to name, so it is stated precisely:

| Part | Shape | Notes |
| --- | --- | --- |
| marker | `66 4C 61 43` | Nothing may precede it except an ID3v2 tag, which some taggers still write |
| metadata header | 1 bit last-block, 7 bits type, 24 bits length | 4 bytes per block |
| `STREAMINFO` (type 0) | mandatory, first, exactly 34 bytes | min/max block size (16+16), min/max frame size (24+24), sample rate (20), channels−1 (3), bits per sample−1 (5), total samples (36), MD5 of the unencoded audio (128) |
| `PADDING` (1) | zeros | length is the only information |
| `APPLICATION` (2) | 4-byte id + payload | opaque, and must stay opaque |
| `SEEKTABLE` (3) | 18 bytes per point | sample number (64), byte offset to the frame (64), samples in that frame (16); an all-ones sample number is a placeholder |
| `VORBIS_COMMENT` (4) | tags, in Vorbis form | where artist/title live |
| `CUESHEET` (5) | CD cue information | absolute sample numbers, not byte offsets |
| `PICTURE` (6) | cover art | can be hundreds of kilobytes |
| frames | to end of file | header (sync, block size, sample rate, channel assignment, sample size, frame/sample number, CRC-8), subframes, padding, CRC-16 |

A frame header carries a **UTF-8-style coded number**: the frame index in a fixed-blocksize stream, or the
first sample number in a variable-blocksize one. Subframes are `CONSTANT`, `VERBATIM`, `FIXED` (orders 0–4)
or `LPC` (orders 1–32), and the residuals are Rice-coded with a per-partition parameter. Stereo may be
decorrelated as independent, left/side, right/side or mid/side.

Two properties of that structure decide the whole contract:

1. **Every offset in the file is a byte offset from the first frame.** A re-encode that changes frame sizes
   moves everything, exactly as in TIFF ([§4.8](guarantees.md#48-tiff-classic-baseline-strips--v07)).
2. **The MD5 in `STREAMINFO` is not a checksum of the file.** It is the hash of the decoded audio: samples
   interleaved, little-endian, signed, in ⌈bits/8⌉ bytes. A re-encode that produces the same samples must
   produce the same MD5 — which turns the format's own field into a verification instrument.

---

## 2. Where the bytes are

| Component | Typical share of the file |
| --- | --- |
| audio frames | 95–99.9 % |
| `PICTURE` (cover art) | 0–5 %, occasionally more |
| `PADDING` | 0–8 KiB fixed, sometimes much more |
| `SEEKTABLE` | tens to hundreds of bytes |
| everything else | tens to hundreds of bytes |

Only the frames can be made smaller without removing metadata. That is the entire opportunity: FLAC is
already an entropy coder, and a modern encoder has squeezed what is in there.

**Honest expected gain.** Files written by `flac -8` (the default of most rippers today) leave roughly
0–1 %: the encoder already searched the predictive orders, the Rice parameters and the stereo mode that the
format allows. Files written by older or faster settings, by hardware recorders, or by tools that chose a
small blocksize (`flac -1` … `-3`) leave 1–3 %, and occasionally more when the original used a fixed
blocksize far from optimal. Anything above 3 % on a modern file would mean the original encoder was bad, not
that Tzim is clever. The formats table therefore says **0–3 %**, and the tool's value here is the same as
everywhere else: it never makes things worse, and it proves what it did.

**`PADDING` is the one exception, and it is not ours to take by default.** It is the only large region of a
FLAC file that carries no information; removing it shrinks the file without touching a sample. It is also
removing a metadata block, which is [G4](guarantees.md#27-g4--metadata-removed-on-request) and therefore
happens only under `--strip-metadata`.

---

## 3. What a re-encode may change

A frame is not a container of samples in any fixed layout: the blocksize, the prediction order, the Rice
parameters, the partition boundaries and the stereo mode are all free choices that the encoder makes. A
different encoder makes different choices, and the file becomes a different sequence of bytes that decodes
to the same audio. That is the definition of a G3 transform
([§2.3](guarantees.md#g3--format-equivalent)), and it means the contract cannot compare frames.

What may change, therefore, is: **every frame, every frame's length, and the offsets in the `SEEKTABLE`.**
Everything else stays.

The four hazards this leaves, in the order they would bite:

1. **`SEEKTABLE` offsets are absolute.** After a re-encode they point into the middle of frames. Each point
   must be re-resolved to the frame that contains its sample number and rewritten; a point that cannot be
   resolved must make the whole file a refusal, not a table with one wrong entry.
2. **The `STREAMINFO` derived fields go stale.** Min/max block size and min/max frame size describe the
   frames. They are allowed to change, and they must be *correct*, or a strict decoder may reject the file
   even though every sample is right.
3. **`PADDING` and `PICTURE` are metadata.** They must be copied byte for byte, and the padding stays
   padding. An "optimizer" that drops them by default is removing information.
4. **Anything before the marker.** An ID3v2 tag in front of `fLaC` is legal in the wild and outside the
   format; it must be preserved verbatim or the file refused. There is no third option.

---

## 4. Encoder and decoder: who writes, who checks

The project rule is that verification must be independent of the writer
([§2.6](guarantees.md#26-verification-independence)), and the rule for dependencies is that no
compression algorithm is hand-written when a vetted crate exists.

| Role | Candidate | Why |
| --- | --- | --- |
| encoder | `flacenc` 0.5.1 | the only maintained pure-Rust FLAC encoder; Apache-2.0; ~10 000 lines of Rust; no C, no `unsafe` requirement, no build script beyond `built` |
| decoder | `claxon` 0.4.3 | pure-Rust FLAC decoder written by a different author, Apache-2.0, ~2 200 lines, no dependencies of its own |
| hash | `md-5` 0.10.6 | RustCrypto, MIT OR Apache-2.0, ~400 lines — and already inside `flacenc`, so it adds no crate weight |

**`flacenc` can also decode, and using it for verification would be wrong.** The point of an independent
decoder is that a bug in the encoder cannot confirm itself; the same crate reading its own output would
have exactly that property, which is why PNG verification uses the `png` crate and not `miniz_oxide`, and
why the JPEG codec has no second decoder at all (and says so). `claxon` is a different implementation by a
different author, which is the strongest claim a second decoder can make.

Hashing is a different matter: MD5 is not a codec, and comparing our own recomputation of the decoded audio
against the value in the *input's* `STREAMINFO` is a check of the audio, not of the encoder.

---

## 5. What verification can prove here

FLAC is, in one respect, the easiest format in the queue: the payload is integers, not pixels.

- **PCM comparison.** Decode both files with `claxon`; compare sample rate, channel count, bits per sample,
  total samples and every sample. No tolerance, no metric, no colour space — byte equality of the decoded
  stream.
- **The format's own MD5.** Recompute MD5 over the decoded audio of the candidate, require it to equal the
  MD5 stored in the candidate's `STREAMINFO` and the MD5 stored in the input's. If the input's MD5 is zero
  ("not computed", which is legal), the PCM comparison still stands on its own.
- **Frame bookkeeping.** Every frame's own CRC-8 and CRC-16 are checked by `claxon` while decoding, so a
  corrupted frame cannot decode "successfully". The coded frame numbers must cover `0…total_samples`
  without gaps or overlaps.

That is a stronger verification than any other format here gets, because there is no float, no
re-quantisation and no colour management anywhere in the chain.

---

## 6. Cost

| Cost | Amount | How it is known |
| --- | --- | --- |
| dependencies | 3 direct crates (`flacenc`, `claxon`, `md-5`), 6 with transitive | `cargo tree` when it lands |
| binary size | `flac`-only build: 836,384 bytes at exact SHA `5967e318` | source-SHA size report [`6521758214`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521758214); this feature-only binary size is not stated as a net delta |
| encoder speed | 6.371 seconds total first-pass time; median 102.685 ms/file | fixed 43-row corpus report [`6521735772`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521735772) |
| memory | one decoded pass per file, bounded by `--max-size` | the existing limit applies unchanged |

`flacenc`'s default features pull `log`, `crossbeam-channel` (for its internal parallelism) and `serde`;
all three are optional and would be switched off (`default-features = false`), because Tzim already
parallelises across files and has its own reporting.

---

## 7. Decision: refused, and the measurement behind it

The audit's first version said "proceed, if the dependencies are approved". That was a decision about *risk*.
The decision that mattered turned out to be about *benefit*, so it was measured before it was made.

### 7.1 What a stronger encoder can buy

Method: `libFLAC` through the `soundfile` bindings, three deterministic 20-second stereo signals at 44.1 kHz
and 16 bits (PCM = 3 528 000 bytes), encoded at every compression level the format defines, `0` (fastest)
through `8` (`flac -8`, what most rippers and every modern convertor use).

| Signal | L0 | L5 (the `flac` default) | L8 (`flac -8`) | L8 vs L5 | L8 vs L0 |
| --- | --- | --- | --- | --- | --- |
| musical material: six harmonics, vibrato, envelopes, a little noise | 2 702 393 | 2 640 081 | 2 629 953 | **−0.38 %** | −2.68 % |
| two-tone signal | 820 596 | 712 583 | 701 494 | −1.56 % | −14.51 % |
| white noise | 3 415 964 | 3 397 373 | 3 397 373 | 0.00 % | −0.54 % |

Read the middle column: it is the *distance between the default encoder and the strongest one*, inside the
same, mature, reference implementation. On material that behaves like music that distance is **0.4 %**; on
real recordings mastered at `-8` it is zero, because there is nothing left to find.

FLAC is not PNG. A PNG has a weak entropy stage — zlib at whatever level the editor happened to use — so
recompressing it with a better filter chain and a stronger encoder finds 5–45 %. FLAC is a linear predictor
followed by Rice coding: the encoder's job is to choose the order and the partition, and both libFLAC and
`flacenc` search the same space. The remaining 0.4 % is the difference between two searches of a space that
is already almost exhausted.

### 7.2 What it would cost

| Cost | Estimate | Basis |
| --- | --- | --- |
| handler code | ~12 000 lines, none of it ours | `flacenc` 0.5.1 (~10 k) + `claxon` 0.4.3 (~2.2 k) |
| binary | **+150…300 KiB**, i.e. 920 KiB → 1.1–1.2 MiB | the repository's own JPEG codec is 1 600 lines and costs 52 KiB; this is 7.5× the code |
| risk | an encoder younger than `libFLAC` by two decades | a worse encoder writes a larger file, the contract refuses it, and the feature does nothing at all |

A quarter of the binary for 0.4 % of a file — and on the files people actually have, for 0 %. That is a worse
trade than any decision recorded in [size-budget §5](size-budget.md#5-options-considered), including the ones
that were rejected for costing 20 KiB.

**The free part is not worth it either.** Dropping `PADDING` (usually 8 KiB) under `--strip-metadata` is 0.03 %
of a 30 MB track. Metadata removal is not a reason to support a format.

### 7.3 What happens to the work that was done

The contract stays ([guarantees §4.9](guarantees.md#49-flac--frame-re-encoding-79-non-default-opt-in)). At the time of this
audit the disposition was to refuse the format; the owner later superseded that decision with #79's opt-in
implementation scope. The analysis and measured numbers remain historical context, not a veto on #79 and not
current size/speed evidence. Until #79's independent runtime verification and acceptance evidence pass, the
format remains unsupported in the binary.

The queue that remains:

| Candidate | State | Why |
| --- | --- | --- |
| zlib / raw DEFLATE | **zlib implemented in v0.9**, raw DEFLATE refused | the zlib container has a signature and a checksum; raw DEFLATE has neither, so it cannot be detected without guessing |
| WOFF2 | struck | Brotli re-encoding cannot be verified without the encoder itself |
| PDF | struck | no verifiable object-level comparison without a renderer |
