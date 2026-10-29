//! Surgical PDF FlateDecode recompression.
//!
//! This intentionally accepts only a single-revision PDF with a classic xref
//! table or an xref stream, direct stream lengths, and unambiguous indirect
//! objects. It copies the object layout and every non-target byte range; only
//! smaller zlib streams, their direct `/Length` integers, the corresponding xref
//! offsets and `startxref` may change. Existing `/ObjStm` objects and their
//! compressed members are preserved verbatim; this handler never generates
//! object streams or serializes the object graph.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::ops::Range;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::error::{Error, Result};

use super::{Cand, Options};

const MAX_HEADER_BYTES: usize = 1024;
const MAX_OBJECTS: usize = 100_000;
const MAX_PDF_VALUE_NODES: usize = 100_000;
const MAX_NESTING: usize = 64;
const MAX_OBJECT_STREAM_BYTES: usize = 8 * 1024 * 1024;
const PDF_EOF: &[u8] = b"%%EOF";

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ObjectId {
    number: u32,
    generation: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Name(Vec<u8>),
    Number(u64),
    Ref(ObjectId),
    Dict(Vec<DictEntry>),
    Array(Vec<Value>),
    Null,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DictEntry {
    key: Vec<u8>,
    value: Value,
    value_range: Range<usize>,
}

#[derive(Clone, Debug)]
struct PdfStream {
    data_range: Range<usize>,
    /// Present when `/Length` is a direct integer in this object.
    length_range: Option<Range<usize>>,
    /// Present when `/Length` is an indirect reference to a direct integer object.
    length_ref: Option<ObjectId>,
    length: usize,
    flate_decode: bool,
}

#[derive(Clone, Debug)]
struct PdfObject {
    id: ObjectId,
    start: usize,
    end: usize,
    value: Value,
    value_range: Range<usize>,
    stream: Option<PdfStream>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct XrefEntry {
    id: ObjectId,
    /// 0 = free, 1 = byte offset, 2 = compressed object-stream member.
    kind: u8,
    field1: u64,
    field2: u64,
}

#[derive(Clone, Debug)]
struct XrefSection {
    first: u32,
    entries: Vec<XrefEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Predictor {
    kind: u8,
    row_bytes: usize,
    bytes_per_pixel: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum XrefCodec {
    Raw,
    Flate(Predictor),
}

#[derive(Clone, Debug)]
struct XrefStreamInfo {
    object_id: ObjectId,
    widths: [usize; 3],
    ranges: Vec<(u32, usize)>,
    codec: XrefCodec,
}

#[derive(Clone, Debug)]
struct PdfDoc {
    header_start: usize,
    header_end: usize,
    xref_start: usize,
    xref_sections: Vec<XrefSection>,
    objects: Vec<PdfObject>,
    xref_object: Option<PdfObject>,
    xref_stream: Option<XrefStreamInfo>,
    trailer_start: usize,
    startxref_start: usize,
    startxref_digits: Range<usize>,
}

#[derive(Clone, Debug)]
struct Patch {
    range: Range<usize>,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct RewritePlan {
    patches: Vec<Patch>,
    xref: Vec<u8>,
    xref_stream_object: Option<Vec<u8>>,
    xref_offset: u64,
    candidate_len: usize,
}

fn refuse(reason: impl std::fmt::Display) -> Error {
    Error::refused(format!("pdf: {reason}; the file is untouched"))
}

fn is_white(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    value_nodes: usize,
    value_node_limit: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self::with_value_node_limit(data, pos, MAX_PDF_VALUE_NODES)
    }

    fn with_value_node_limit(data: &'a [u8], pos: usize, value_node_limit: usize) -> Self {
        Self {
            data,
            pos,
            value_nodes: 0,
            value_node_limit,
        }
    }

    fn skip_space_comments(&mut self) {
        loop {
            while self.pos < self.data.len() && is_white(self.data[self.pos]) {
                self.pos += 1;
            }
            if self.pos < self.data.len() && self.data[self.pos] == b'%' {
                while self.pos < self.data.len() && !matches!(self.data[self.pos], b'\r' | b'\n') {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn starts(&self, token: &[u8]) -> bool {
        self.data
            .get(self.pos..self.pos.saturating_add(token.len()))
            == Some(token)
    }

    fn keyword(&mut self, token: &[u8]) -> bool {
        if !self.starts(token) {
            return false;
        }
        let end = self.pos + token.len();
        if end < self.data.len() && !is_white(self.data[end]) && !is_delimiter(self.data[end]) {
            return false;
        }
        self.pos = end;
        true
    }

    fn expect_keyword(&mut self, token: &[u8], what: &str) -> Result<()> {
        if self.keyword(token) {
            Ok(())
        } else {
            Err(Error::parse(format!("pdf: expected {what}")))
        }
    }

    fn atom(&mut self) -> Result<Vec<u8>> {
        let start = self.pos;
        while self.pos < self.data.len()
            && !is_white(self.data[self.pos])
            && !is_delimiter(self.data[self.pos])
        {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(Error::parse("pdf: expected a token"));
        }
        Ok(self.data[start..self.pos].to_vec())
    }

    fn name(&mut self) -> Result<Vec<u8>> {
        if !self.starts(b"/") {
            return Err(Error::parse("pdf: expected a name"));
        }
        self.pos += 1;
        let mut name = Vec::new();
        while self.pos < self.data.len()
            && !is_white(self.data[self.pos])
            && !is_delimiter(self.data[self.pos])
        {
            let byte = self.data[self.pos];
            if byte == b'#' {
                let hi = self.data.get(self.pos + 1).copied().and_then(hex);
                let lo = self.data.get(self.pos + 2).copied().and_then(hex);
                let (Some(hi), Some(lo)) = (hi, lo) else {
                    return Err(Error::parse("pdf: malformed # escape in a name"));
                };
                name.push((hi << 4) | lo);
                self.pos += 3;
            } else {
                name.push(byte);
                self.pos += 1;
            }
        }
        Ok(name)
    }

    fn literal_string(&mut self) -> Result<()> {
        self.pos += 1; // opening parenthesis
        let mut depth = 1usize;
        while self.pos < self.data.len() {
            match self.data[self.pos] {
                b'\\' => {
                    self.pos += 1;
                    if self.pos < self.data.len() {
                        if self.data[self.pos] == b'\r'
                            && self.data.get(self.pos + 1) == Some(&b'\n')
                        {
                            self.pos += 2;
                        } else {
                            self.pos += 1;
                        }
                    }
                }
                b'(' => {
                    depth += 1;
                    if depth > MAX_NESTING {
                        return Err(refuse("literal string nesting exceeds 64"));
                    }
                    self.pos += 1;
                }
                b')' => {
                    self.pos += 1;
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => self.pos += 1,
            }
        }
        Err(Error::parse("pdf: unterminated literal string"))
    }

    fn hex_string(&mut self) -> Result<()> {
        self.pos += 1; // opening '<'
        while self.pos < self.data.len() {
            if self.data[self.pos] == b'>' {
                self.pos += 1;
                return Ok(());
            }
            if !is_white(self.data[self.pos]) && hex(self.data[self.pos]).is_none() {
                return Err(Error::parse("pdf: invalid byte in hexadecimal string"));
            }
            self.pos += 1;
        }
        Err(Error::parse("pdf: unterminated hexadecimal string"))
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_NESTING {
            return Err(refuse("PDF object nesting exceeds 64"));
        }
        if self.value_nodes >= self.value_node_limit {
            return Err(refuse("PDF parser exceeds the 100,000-value-node limit"));
        }
        self.value_nodes += 1;
        self.skip_space_comments();
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| Error::parse("pdf: expected an object value"))?;
        match byte {
            b'/' => Ok(Value::Name(self.name()?)),
            b'(' => {
                self.literal_string()?;
                Ok(Value::Other)
            }
            b'<' if self.data.get(self.pos + 1) == Some(&b'<') => self.dictionary(depth + 1),
            b'<' => {
                self.hex_string()?;
                Ok(Value::Other)
            }
            b'[' => self.array(depth + 1),
            b']' | b'>' | b')' => Err(Error::parse("pdf: unexpected closing delimiter")),
            _ => {
                let token_start = self.pos;
                let token = self.atom()?;
                if token == b"null" {
                    return Ok(Value::Null);
                }
                if let Ok(value) = std::str::from_utf8(&token).unwrap_or("").parse::<i64>() {
                    let number_end = self.pos;
                    self.skip_space_comments();
                    let second = self.atom().ok();
                    if let Some(second) = second {
                        if let (Ok(number), Ok(generation)) = (
                            std::str::from_utf8(&token).unwrap_or("").parse::<u64>(),
                            std::str::from_utf8(&second).unwrap_or("").parse::<u64>(),
                        ) {
                            self.skip_space_comments();
                            if self.keyword(b"R") {
                                if number > u32::MAX as u64 || generation > u16::MAX as u64 {
                                    return Err(refuse(
                                        "indirect reference exceeds PDF object limits",
                                    ));
                                }
                                return Ok(Value::Ref(ObjectId {
                                    number: number as u32,
                                    generation: generation as u16,
                                }));
                            }
                        }
                    }
                    self.pos = number_end;
                    if value < 0 {
                        return Ok(Value::Other);
                    }
                    Ok(Value::Number(value as u64))
                } else {
                    self.pos = token_start + token.len();
                    Ok(Value::Other)
                }
            }
        }
    }

    fn dictionary(&mut self, depth: usize) -> Result<Value> {
        self.pos += 2;
        let mut entries = Vec::new();
        let mut keys = HashSet::new();
        loop {
            self.skip_space_comments();
            if self.starts(b">>") {
                self.pos += 2;
                return Ok(Value::Dict(entries));
            }
            if entries.len() >= 100_000 {
                return Err(refuse("dictionary has more than 100,000 keys"));
            }
            if self.pos >= self.data.len() {
                return Err(Error::parse("pdf: unterminated dictionary"));
            }
            let key = self.name()?;
            if !keys.insert(key.clone()) {
                return Err(refuse("duplicate key in a dictionary"));
            }
            self.skip_space_comments();
            let value_start = self.pos;
            let value = self.parse_value(depth)?;
            let value_end = self.pos;
            entries.push(DictEntry {
                key,
                value,
                value_range: value_start..value_end,
            });
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        self.pos += 1;
        let mut values = Vec::new();
        loop {
            self.skip_space_comments();
            if self.data.get(self.pos) == Some(&b']') {
                self.pos += 1;
                return Ok(Value::Array(values));
            }
            if self.pos >= self.data.len() {
                return Err(Error::parse("pdf: unterminated array"));
            }
            if values.len() >= 100_000 {
                return Err(refuse("array has more than 100,000 values"));
            }
            values.push(self.parse_value(depth)?);
        }
    }
}

fn dict_entry<'a>(value: &'a Value, key: &[u8]) -> Option<&'a DictEntry> {
    let Value::Dict(entries) = value else {
        return None;
    };
    entries.iter().find(|entry| entry.key == key)
}

fn is_flate_filter(value: &Value) -> bool {
    match value {
        Value::Name(name) => name.as_slice() == b"FlateDecode",
        Value::Array(values) if values.len() == 1 => {
            matches!(&values[0], Value::Name(name) if name.as_slice() == b"FlateDecode")
        }
        _ => false,
    }
}

fn contains_key(value: &Value, key: &[u8]) -> bool {
    match value {
        Value::Dict(entries) => entries
            .iter()
            .any(|entry| entry.key == key || contains_key(&entry.value, key)),
        Value::Array(values) => values.iter().any(|value| contains_key(value, key)),
        _ => false,
    }
}

fn valid_gap(data: &[u8], range: Range<usize>) -> bool {
    let mut parser = Parser::new(data, range.start);
    parser.skip_space_comments();
    parser.pos == range.end
}

fn read_line(data: &[u8], pos: &mut usize) -> Option<Range<usize>> {
    if *pos >= data.len() {
        return None;
    }
    let start = *pos;
    while *pos < data.len() && !matches!(data[*pos], b'\r' | b'\n') {
        *pos += 1;
    }
    let end = *pos;
    if data.get(*pos) == Some(&b'\r') {
        *pos += 1;
        if data.get(*pos) == Some(&b'\n') {
            *pos += 1;
        }
    } else if data.get(*pos) == Some(&b'\n') {
        *pos += 1;
    }
    Some(start..end)
}

fn trimmed(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(start, |index| index + 1);
    &line[start..end]
}

fn parse_xref_line(line: &[u8]) -> Result<(u64, u16, u8)> {
    let mut fields = trimmed(line)
        .split(|byte| matches!(byte, b' ' | b'\t'))
        .filter(|field| !field.is_empty());
    let (Some(offset), Some(generation), Some(status)) =
        (fields.next(), fields.next(), fields.next())
    else {
        return Err(Error::parse("pdf: malformed classic xref entry"));
    };
    if fields.next().is_some() {
        return Err(Error::parse("pdf: malformed classic xref entry"));
    }
    let field = std::str::from_utf8(offset)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| Error::parse("pdf: invalid xref offset or free-list pointer"))?;
    let generation = std::str::from_utf8(generation)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| Error::parse("pdf: invalid xref generation"))?;
    let kind = match status {
        b"n" => 1,
        b"f" => 0,
        _ => return Err(Error::parse("pdf: xref status must be n or f")),
    };
    Ok((field, generation, kind))
}

fn parse_xref(
    data: &[u8],
    xref_start: usize,
    value_node_limit: usize,
) -> Result<(Vec<XrefSection>, usize, Value, usize, usize)> {
    let mut pos = xref_start;
    if data.get(pos..pos.saturating_add(4)) != Some(b"xref") {
        return Err(refuse(
            "xref streams are outside the accepted classic-table subset",
        ));
    }
    pos += 4;
    if data.get(pos) == Some(&b'\r') {
        pos += 1;
        if data.get(pos) == Some(&b'\n') {
            pos += 1;
        }
    } else if data.get(pos) == Some(&b'\n') {
        pos += 1;
    } else {
        return Err(Error::parse(
            "pdf: classic xref keyword is not line-terminated",
        ));
    }

    let mut sections = Vec::new();
    let mut seen = HashSet::new();
    let mut total_entries = 0usize;
    let trailer_start;
    loop {
        let line = read_line(data, &mut pos)
            .ok_or_else(|| Error::parse("pdf: xref table has no trailer"))?;
        let text = trimmed(&data[line.clone()]);
        if text.is_empty() {
            continue;
        }
        if text == b"trailer" {
            trailer_start = line.start;
            break;
        }
        let mut fields = text
            .split(|byte| matches!(byte, b' ' | b'\t'))
            .filter(|field| !field.is_empty());
        let (Some(first), Some(count)) = (fields.next(), fields.next()) else {
            return Err(Error::parse("pdf: malformed xref subsection header"));
        };
        if fields.next().is_some() {
            return Err(Error::parse("pdf: malformed xref subsection header"));
        }
        let first = std::str::from_utf8(first)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| Error::parse("pdf: invalid xref subsection start"))?;
        let count = std::str::from_utf8(count)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| Error::parse("pdf: invalid xref subsection count"))?;
        if count == 0 || count > MAX_OBJECTS || total_entries.saturating_add(count) > MAX_OBJECTS {
            return Err(refuse("xref table exceeds the 100,000-object limit"));
        }
        total_entries += count;
        let last = u64::from(first) + count as u64;
        if last > u64::from(u32::MAX) + 1 {
            return Err(refuse("xref subsection object number overflows"));
        }
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let row = read_line(data, &mut pos)
                .ok_or_else(|| Error::parse("pdf: truncated xref subsection"))?;
            let (field1, generation, kind) = parse_xref_line(&data[row])?;
            let number = first + index as u32;
            let id = ObjectId { number, generation };
            if !seen.insert(number) {
                return Err(refuse("object number appears in more than one xref entry"));
            }
            if kind == 1 && field1 >= xref_start as u64 {
                return Err(Error::parse(
                    "pdf: in-use xref offset points beyond the xref table",
                ));
            }
            entries.push(XrefEntry {
                id,
                kind,
                field1,
                field2: u64::from(generation),
            });
        }
        sections.push(XrefSection { first, entries });
    }

    let mut trailer_parser = Parser::with_value_node_limit(data, pos, value_node_limit);
    trailer_parser.skip_space_comments();
    let trailer_value = trailer_parser.parse_value(0)?;
    if !matches!(trailer_value, Value::Dict(_)) {
        return Err(Error::parse("pdf: trailer is not a dictionary"));
    }
    let trailer_end = trailer_parser.pos;
    Ok((
        sections,
        trailer_start,
        trailer_value,
        trailer_end,
        trailer_parser.value_nodes,
    ))
}

fn parse_numeric_object(
    data: &[u8],
    offset: usize,
) -> Result<Option<(ObjectId, u64, Range<usize>)>> {
    let mut parser = Parser::new(data, offset);
    parser.skip_space_comments();
    let start = parser.pos;
    if start != offset {
        return Ok(None);
    }
    let Ok(number_token) = parser.atom() else {
        return Ok(None);
    };
    let Some(number) = std::str::from_utf8(&number_token)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    parser.skip_space_comments();
    let Ok(generation_token) = parser.atom() else {
        return Ok(None);
    };
    let Some(generation) = std::str::from_utf8(&generation_token)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
    else {
        return Ok(None);
    };
    parser.skip_space_comments();
    if !parser.keyword(b"obj") {
        return Ok(None);
    }
    parser.skip_space_comments();
    let value_start = parser.pos;
    let Ok(value_token) = parser.atom() else {
        return Ok(None);
    };
    let Some(value) = std::str::from_utf8(&value_token)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return Ok(None);
    };
    let value_end = parser.pos;
    parser.skip_space_comments();
    if !parser.keyword(b"endobj") {
        return Ok(None);
    }
    Ok(Some((
        ObjectId { number, generation },
        value,
        value_start..value_end,
    )))
}

fn parse_object_with_lengths(
    data: &[u8],
    offset: usize,
    length_values: &HashMap<ObjectId, u64>,
    value_node_limit: usize,
) -> Result<(PdfObject, usize)> {
    let mut parser = Parser::with_value_node_limit(data, offset, value_node_limit);
    parser.skip_space_comments();
    let start = parser.pos;
    if start != offset {
        return Err(Error::parse(
            "pdf: xref offset does not point directly to an object header",
        ));
    }
    let number_token = parser.atom()?;
    let number = std::str::from_utf8(&number_token)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| Error::parse("pdf: invalid object number"))?;
    parser.skip_space_comments();
    let generation_token = parser.atom()?;
    let generation = std::str::from_utf8(&generation_token)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| Error::parse("pdf: invalid object generation"))?;
    parser.skip_space_comments();
    parser.expect_keyword(b"obj", "obj after object number")?;
    parser.skip_space_comments();
    let value_start = parser.pos;
    let value = parser.parse_value(0)?;
    let value_range = value_start..parser.pos;

    let mut stream = None;
    parser.skip_space_comments();
    if parser.keyword(b"stream") {
        let Value::Dict(entries) = &value else {
            return Err(Error::parse("pdf: stream object has no dictionary"));
        };
        if entries.iter().any(|entry| entry.key == b"F") {
            return Err(refuse("external-file stream key /F"));
        }
        let length_entry =
            dict_entry(&value, b"Length").ok_or_else(|| refuse("stream has no /Length"))?;
        let (length, length_range, length_ref) = match &length_entry.value {
            Value::Number(length) => (*length, Some(length_entry.value_range.clone()), None),
            Value::Ref(id) => (
                *length_values
                    .get(id)
                    .ok_or_else(|| refuse("indirect /Length is not a direct integer object"))?,
                None,
                Some(id.clone()),
            ),
            _ => return Err(refuse("non-integer stream /Length")),
        };
        let length = usize::try_from(length)
            .map_err(|_| refuse("stream length does not fit this platform"))?;
        if parser.data.get(parser.pos) == Some(&b'\r') {
            parser.pos += 1;
            if parser.data.get(parser.pos) == Some(&b'\n') {
                parser.pos += 1;
            }
        } else if parser.data.get(parser.pos) == Some(&b'\n') {
            parser.pos += 1;
        } else {
            return Err(Error::parse(
                "pdf: stream keyword is not followed by an end-of-line",
            ));
        }
        let data_start = parser.pos;
        let data_end = data_start
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| Error::parse("pdf: stream /Length runs past end of file"))?;
        parser.pos = data_end;
        while parser.pos < data.len() && is_white(data[parser.pos]) {
            parser.pos += 1;
        }
        parser.expect_keyword(b"endstream", "endstream at /Length boundary")?;
        let filter =
            dict_entry(&value, b"Filter").is_some_and(|entry| is_flate_filter(&entry.value));
        stream = Some(PdfStream {
            data_range: data_start..data_end,
            length_range,
            length_ref,
            length,
            flate_decode: filter,
        });
    }
    parser.skip_space_comments();
    parser.expect_keyword(b"endobj", "endobj")?;
    let value_nodes = parser.value_nodes;
    Ok((
        PdfObject {
            id: ObjectId { number, generation },
            start,
            end: parser.pos,
            value,
            value_range,
            stream,
        },
        value_nodes,
    ))
}

fn number_entry(value: &Value, key: &[u8]) -> Option<u64> {
    dict_entry(value, key).and_then(|entry| match &entry.value {
        Value::Number(number) => Some(*number),
        _ => None,
    })
}

fn zlib_decode(bytes: &[u8], what: &str, output_limit: usize) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    let mut decoded = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = decoder
            .read(&mut buffer)
            .map_err(|error| refuse(format!("{what} zlib stream is invalid: {error}")))?;
        if count == 0 {
            break;
        }
        if decoded.len().saturating_add(count) > output_limit {
            return Err(refuse(format!(
                "{what} expands beyond its bounded decoded-size limit"
            )));
        }
        decoded.extend_from_slice(&buffer[..count]);
    }
    if decoder.total_in() != bytes.len() as u64 {
        return Err(refuse(format!("{what} zlib stream has trailing bytes")));
    }
    Ok(decoded)
}

fn zlib_encode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(bytes)
        .map_err(|error| Error::Internal(format!("PDF xref zlib encoding failed: {error}")))?;
    encoder
        .finish()
        .map_err(|error| Error::Internal(format!("PDF xref zlib finalization failed: {error}")))
}

fn paeth(left: u8, up: u8, upper_left: u8) -> u8 {
    let p = i32::from(left) + i32::from(up) - i32::from(upper_left);
    let dl = (p - i32::from(left)).abs();
    let du = (p - i32::from(up)).abs();
    let dul = (p - i32::from(upper_left)).abs();
    if dl <= du && dl <= dul {
        left
    } else if du <= dul {
        up
    } else {
        upper_left
    }
}

fn direct_number(value: &Value, key: &[u8], default: u64) -> Result<u64> {
    match dict_entry(value, key).map(|entry| &entry.value) {
        None => Ok(default),
        Some(Value::Number(number)) => Ok(*number),
        Some(_) => Err(refuse(format!(
            "xref stream /DecodeParms /{} is not an integer",
            String::from_utf8_lossy(key)
        ))),
    }
}

fn predictor_from_dict(value: &Value, row_bytes: usize) -> Result<Predictor> {
    let full = dict_entry(value, b"DecodeParms");
    let abbreviated = dict_entry(value, b"DP");
    if full.is_some() && abbreviated.is_some() {
        return Err(refuse("xref stream has both /DecodeParms and /DP"));
    }
    let entry = full.or(abbreviated);
    let params = match entry.map(|entry| &entry.value) {
        None | Some(Value::Null) => None,
        Some(Value::Dict(_)) => entry.map(|entry| &entry.value),
        Some(Value::Array(values)) if values.len() == 1 => match &values[0] {
            Value::Null => None,
            Value::Dict(_) => Some(&values[0]),
            _ => return Err(refuse("xref stream /DecodeParms array is not a dictionary")),
        },
        _ => {
            return Err(refuse(
                "xref stream /DecodeParms is not a direct dictionary",
            ))
        }
    };
    let (kind, colors, bits, columns) = if let Some(params) = params {
        (
            direct_number(params, b"Predictor", 1)?,
            direct_number(params, b"Colors", 1)?,
            direct_number(params, b"BitsPerComponent", 8)?,
            direct_number(params, b"Columns", 1)?,
        )
    } else {
        (1, 1, 8, row_bytes as u64)
    };
    if kind == 1 {
        return Ok(Predictor {
            kind: 1,
            row_bytes,
            bytes_per_pixel: 1,
        });
    }
    if !(kind == 2 || (10..=15).contains(&kind)) {
        return Err(refuse(format!(
            "xref stream predictor {kind} is unsupported"
        )));
    }
    if colors != 1 || bits != 8 || columns != row_bytes as u64 {
        return Err(refuse(
            "xref stream predictor must use one 8-bit component and Columns equal to the xref row width",
        ));
    }
    Ok(Predictor {
        kind: kind as u8,
        row_bytes,
        bytes_per_pixel: 1,
    })
}

fn predictor_decode(input: &[u8], predictor: &Predictor) -> Result<Vec<u8>> {
    let row = predictor.row_bytes;
    if row == 0 {
        return Err(refuse("xref stream has a zero-width row"));
    }
    match predictor.kind {
        1 => Ok(input.to_vec()),
        2 => {
            if !input.len().is_multiple_of(row) {
                return Err(refuse(
                    "TIFF-predicted xref data is not a whole number of rows",
                ));
            }
            let mut output = input.to_vec();
            for chunk in output.chunks_mut(row) {
                for index in predictor.bytes_per_pixel..row {
                    chunk[index] =
                        chunk[index].wrapping_add(chunk[index - predictor.bytes_per_pixel]);
                }
            }
            Ok(output)
        }
        10..=15 => {
            let encoded_row = row + 1;
            if !input.len().is_multiple_of(encoded_row) {
                return Err(refuse(
                    "PNG-predicted xref data is not a whole number of rows",
                ));
            }
            let mut output = Vec::with_capacity(input.len() / encoded_row * row);
            let mut previous = vec![0u8; row];
            for raw in input.chunks_exact(encoded_row) {
                let filter = raw[0];
                let expected = if predictor.kind == 15 {
                    None
                } else {
                    Some(predictor.kind - 10)
                };
                if filter > 4 || expected.is_some_and(|value| value != filter) {
                    return Err(refuse("PNG xref row uses an invalid predictor filter"));
                }
                let mut current = vec![0u8; row];
                for index in 0..row {
                    let left = if index >= predictor.bytes_per_pixel {
                        current[index - predictor.bytes_per_pixel]
                    } else {
                        0
                    };
                    let up = previous[index];
                    let upper_left = if index >= predictor.bytes_per_pixel {
                        previous[index - predictor.bytes_per_pixel]
                    } else {
                        0
                    };
                    let add = match filter {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                        4 => paeth(left, up, upper_left),
                        _ => unreachable!(),
                    };
                    current[index] = raw[index + 1].wrapping_add(add);
                }
                output.extend_from_slice(&current);
                previous = current;
            }
            Ok(output)
        }
        _ => Err(refuse("unsupported xref stream predictor")),
    }
}

fn predictor_encode(input: &[u8], predictor: &Predictor) -> Result<Vec<u8>> {
    let row = predictor.row_bytes;
    if row == 0 || !input.len().is_multiple_of(row) {
        return Err(refuse("xref data is not a whole number of predictor rows"));
    }
    match predictor.kind {
        1 => Ok(input.to_vec()),
        2 => {
            let mut output = input.to_vec();
            for chunk in output.chunks_mut(row) {
                for index in (predictor.bytes_per_pixel..row).rev() {
                    chunk[index] =
                        chunk[index].wrapping_sub(chunk[index - predictor.bytes_per_pixel]);
                }
            }
            Ok(output)
        }
        10..=14 => {
            let filter = predictor.kind - 10;
            let mut output = Vec::with_capacity(input.len() + input.len() / row);
            let mut previous = vec![0u8; row];
            for current in input.chunks_exact(row) {
                output.push(filter);
                for index in 0..row {
                    let left = if index >= predictor.bytes_per_pixel {
                        current[index - predictor.bytes_per_pixel]
                    } else {
                        0
                    };
                    let up = previous[index];
                    let upper_left = if index >= predictor.bytes_per_pixel {
                        previous[index - predictor.bytes_per_pixel]
                    } else {
                        0
                    };
                    let sub = match filter {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                        4 => paeth(left, up, upper_left),
                        _ => unreachable!(),
                    };
                    output.push(current[index].wrapping_sub(sub));
                }
                previous.copy_from_slice(current);
            }
            Ok(output)
        }
        15 => {
            let mut output = Vec::with_capacity(input.len() + input.len() / row);
            for current in input.chunks_exact(row) {
                output.push(0);
                output.extend_from_slice(current);
            }
            Ok(output)
        }
        _ => Err(refuse("unsupported xref stream predictor")),
    }
}

fn stream_codec(value: &Value, row_bytes: usize, what: &str) -> Result<XrefCodec> {
    match dict_entry(value, b"Filter").map(|entry| &entry.value) {
        None => {
            let has_parameters = dict_entry(value, b"DecodeParms")
                .is_some_and(|entry| !matches!(&entry.value, Value::Null))
                || dict_entry(value, b"DP")
                    .is_some_and(|entry| !matches!(&entry.value, Value::Null));
            if has_parameters {
                return Err(refuse(format!("{what} has /DecodeParms without a filter")));
            }
            Ok(XrefCodec::Raw)
        }
        Some(filter) if is_flate_filter(filter) => {
            Ok(XrefCodec::Flate(predictor_from_dict(value, row_bytes)?))
        }
        _ => Err(refuse(format!(
            "{what} filter is not a single /FlateDecode"
        ))),
    }
}

fn decode_with_codec(
    bytes: &[u8],
    codec: &XrefCodec,
    what: &str,
    output_limit: usize,
) -> Result<Vec<u8>> {
    match codec {
        XrefCodec::Raw => {
            if bytes.len() > output_limit {
                return Err(refuse(format!(
                    "{what} exceeds its bounded decoded-size limit"
                )));
            }
            Ok(bytes.to_vec())
        }
        XrefCodec::Flate(predictor) => {
            predictor_decode(&zlib_decode(bytes, what, output_limit)?, predictor)
        }
    }
}

fn encode_with_codec(bytes: &[u8], codec: &XrefCodec) -> Result<Vec<u8>> {
    match codec {
        XrefCodec::Raw => Ok(bytes.to_vec()),
        XrefCodec::Flate(predictor) => zlib_encode(&predictor_encode(bytes, predictor)?),
    }
}

fn parse_xref_stream(
    data: &[u8],
    offset: usize,
    value_node_limit: usize,
) -> Result<(PdfObject, Vec<XrefSection>, XrefStreamInfo, usize)> {
    let (object, value_nodes) =
        parse_object_with_lengths(data, offset, &HashMap::new(), value_node_limit)?;
    if !name_is(
        dict_entry(&object.value, b"Type").map(|entry| &entry.value),
        b"XRef",
    ) {
        return Err(refuse(
            "startxref does not point to a classic table or /Type /XRef stream",
        ));
    }
    let stream = object
        .stream
        .as_ref()
        .ok_or_else(|| refuse("/Type /XRef object has no stream"))?;
    let width_values = match dict_entry(&object.value, b"W").map(|entry| &entry.value) {
        Some(Value::Array(values)) if values.len() == 3 => values,
        _ => return Err(refuse("xref stream /W is not a three-element array")),
    };
    let mut widths = [0usize; 3];
    for (index, value) in width_values.iter().enumerate() {
        let Value::Number(width) = value else {
            return Err(refuse("xref stream /W contains a non-integer"));
        };
        widths[index] =
            usize::try_from(*width).map_err(|_| refuse("xref field width overflows"))?;
        if widths[index] > 8 {
            return Err(refuse("xref stream field width exceeds eight bytes"));
        }
    }
    let row_bytes = widths.iter().sum::<usize>();
    if row_bytes == 0 || row_bytes > 24 {
        return Err(refuse("xref stream row width is outside 1..=24 bytes"));
    }
    let size = number_entry(&object.value, b"Size")
        .ok_or_else(|| Error::parse("pdf: xref stream /Size is missing or invalid"))?;
    if size == 0 || size > (MAX_OBJECTS as u64 + 1) {
        return Err(refuse("xref stream /Size exceeds the 100,000-object limit"));
    }
    let ranges = match dict_entry(&object.value, b"Index").map(|entry| &entry.value) {
        None => vec![(
            0,
            usize::try_from(size).map_err(|_| refuse("xref /Size overflows"))?,
        )],
        Some(Value::Array(values)) if values.len().is_multiple_of(2) => {
            let mut ranges = Vec::with_capacity(values.len() / 2);
            for pair in values.as_chunks::<2>().0 {
                let (Value::Number(first), Value::Number(count)) = (&pair[0], &pair[1]) else {
                    return Err(refuse("xref stream /Index contains a non-integer"));
                };
                let first =
                    u32::try_from(*first).map_err(|_| refuse("xref /Index start overflows"))?;
                let count =
                    usize::try_from(*count).map_err(|_| refuse("xref /Index count overflows"))?;
                if count == 0 || u64::from(first) + count as u64 > size {
                    return Err(refuse("xref stream /Index range is empty or exceeds /Size"));
                }
                ranges.push((first, count));
            }
            ranges
        }
        _ => return Err(refuse("xref stream /Index is not an even integer array")),
    };
    let mut total_entries = 0usize;
    let mut previous_end = 0u64;
    for (first, count) in &ranges {
        if u64::from(*first) < previous_end {
            return Err(refuse(
                "xref stream /Index ranges overlap or are out of order",
            ));
        }
        previous_end = u64::from(*first) + *count as u64;
        total_entries = total_entries.saturating_add(*count);
        if total_entries > MAX_OBJECTS {
            return Err(refuse("xref stream exceeds the 100,000-entry limit"));
        }
    }
    let codec = stream_codec(&object.value, row_bytes, "xref stream")?;
    let expected = total_entries
        .checked_mul(row_bytes)
        .ok_or_else(|| refuse("xref stream decoded length overflows"))?;
    let predictor_prefixes = match &codec {
        XrefCodec::Flate(predictor) if (10..=15).contains(&predictor.kind) => total_entries,
        _ => 0,
    };
    let encoded_output_limit = expected
        .checked_add(predictor_prefixes)
        .ok_or_else(|| refuse("xref stream predictor size overflows"))?;
    let decoded = decode_with_codec(
        &data[stream.data_range.clone()],
        &codec,
        "xref stream",
        encoded_output_limit,
    )?;
    if decoded.len() != expected {
        return Err(refuse(format!(
            "xref stream decoded to {} bytes, expected {expected}",
            decoded.len()
        )));
    }

    fn read_field(row: &[u8], offset: &mut usize, width: usize, default: u64) -> Result<u64> {
        if width == 0 {
            return Ok(default);
        }
        let end = offset
            .checked_add(width)
            .filter(|end| *end <= row.len())
            .ok_or_else(|| Error::parse("pdf: truncated xref stream row"))?;
        let mut value = 0u64;
        for byte in &row[*offset..end] {
            value = value
                .checked_mul(256)
                .and_then(|value| value.checked_add(u64::from(*byte)))
                .ok_or_else(|| refuse("xref stream field overflows"))?;
        }
        *offset = end;
        Ok(value)
    }

    let mut sections = Vec::with_capacity(ranges.len());
    let mut seen = HashSet::new();
    let mut row_index = 0usize;
    for (first, count) in &ranges {
        let mut entries = Vec::with_capacity(*count);
        for index in 0..*count {
            let start = row_index * row_bytes;
            let row = &decoded[start..start + row_bytes];
            row_index += 1;
            let mut position = 0usize;
            let kind = read_field(row, &mut position, widths[0], 1)?;
            let field1 = read_field(row, &mut position, widths[1], 0)?;
            let field2 = read_field(row, &mut position, widths[2], 0)?;
            if position != row_bytes || kind > 2 {
                return Err(refuse("xref stream contains an unsupported entry type"));
            }
            let number = first
                .checked_add(index as u32)
                .ok_or_else(|| refuse("xref object number overflows"))?;
            if !seen.insert(number) {
                return Err(refuse(
                    "object number appears in more than one xref /Index range",
                ));
            }
            let generation = match kind {
                0 | 1 => {
                    u16::try_from(field2).map_err(|_| refuse("xref generation exceeds 65535"))?
                }
                2 => 0,
                _ => unreachable!(),
            };
            if kind == 2 && (field1 > u32::MAX as u64 || field2 > MAX_OBJECTS as u64) {
                return Err(refuse(
                    "compressed-object xref reference exceeds parser limits",
                ));
            }
            entries.push(XrefEntry {
                id: ObjectId { number, generation },
                kind: kind as u8,
                field1,
                field2,
            });
        }
        sections.push(XrefSection {
            first: *first,
            entries,
        });
    }
    Ok((
        object.clone(),
        sections,
        XrefStreamInfo {
            object_id: object.id,
            widths,
            ranges,
            codec,
        },
        value_nodes,
    ))
}

fn object_stream_predictor_is_identity(params: &[DictEntry]) -> bool {
    params.iter().all(|entry| {
        matches!(
            entry.key.as_slice(),
            b"Predictor" | b"Colors" | b"BitsPerComponent" | b"Columns"
        ) && matches!(&entry.value, Value::Number(_))
    }) && params
        .iter()
        .find(|entry| entry.key == b"Predictor")
        .is_none_or(|entry| matches!(&entry.value, Value::Number(1)))
}

fn object_stream_decode_parms_supported(value: &Value) -> bool {
    let decode_parms = dict_entry(value, b"DecodeParms");
    let abbreviated = dict_entry(value, b"DP");
    if decode_parms.is_some() && abbreviated.is_some() {
        return false;
    }
    match decode_parms.or(abbreviated).map(|entry| &entry.value) {
        None | Some(Value::Null) => true,
        Some(Value::Dict(params)) => object_stream_predictor_is_identity(params),
        Some(Value::Array(values)) if values.len() == 1 => match &values[0] {
            Value::Null => true,
            Value::Dict(params) => object_stream_predictor_is_identity(params),
            _ => false,
        },
        _ => false,
    }
}

fn object_stream_xref_index(entry: &XrefEntry, members: &[u32]) -> Result<usize> {
    let index = usize::try_from(entry.field2)
        .map_err(|_| refuse("object-stream index does not fit this platform"))?;
    if index >= members.len() {
        return Err(refuse("compressed-object xref index is outside /ObjStm /N"));
    }
    if members[index] != entry.id.number {
        return Err(refuse(
            "compressed-object xref number does not match the /ObjStm header",
        ));
    }
    Ok(index)
}

fn object_stream_member_ids(
    data: &[u8],
    object: &PdfObject,
    value_node_limit: usize,
) -> Result<(Vec<u32>, usize)> {
    if !name_is(
        dict_entry(&object.value, b"Type").map(|entry| &entry.value),
        b"ObjStm",
    ) {
        return Err(refuse("xref type-2 entry does not name an /ObjStm object"));
    }
    let count = number_entry(&object.value, b"N")
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count > 0 && *count <= MAX_OBJECTS)
        .ok_or_else(|| refuse("object stream /N is missing or outside 1..=100,000"))?;
    let first = number_entry(&object.value, b"First")
        .and_then(|first| usize::try_from(first).ok())
        .filter(|first| *first <= u32::MAX as usize)
        .ok_or_else(|| refuse("object stream /First is missing or invalid"))?;
    let stream = object
        .stream
        .as_ref()
        .ok_or_else(|| refuse("/Type /ObjStm object has no stream"))?;
    if object.id.generation != 0 {
        return Err(refuse("object stream generation is not zero"));
    }
    if dict_entry(&object.value, b"F").is_some() || dict_entry(&object.value, b"Extends").is_some()
    {
        return Err(refuse("object stream uses external-file data or /Extends"));
    }
    let encoded = &data[stream.data_range.clone()];
    let decoded = match dict_entry(&object.value, b"Filter").map(|entry| &entry.value) {
        None => {
            if !object_stream_decode_parms_supported(&object.value) {
                return Err(refuse(
                    "unfiltered object stream has unsupported /DecodeParms",
                ));
            }
            if encoded.len() > MAX_OBJECT_STREAM_BYTES {
                return Err(refuse("object stream exceeds the 8 MiB decoded-size limit"));
            }
            encoded.to_vec()
        }
        Some(filter) if is_flate_filter(filter) => {
            if !object_stream_decode_parms_supported(&object.value) {
                return Err(refuse("object stream /DecodeParms is unsupported"));
            }
            zlib_decode(encoded, "object stream", MAX_OBJECT_STREAM_BYTES)?
        }
        _ => return Err(refuse("object stream filter is not a single /FlateDecode")),
    };
    if first > decoded.len() {
        return Err(refuse("object stream /First lies beyond the decoded bytes"));
    }

    let mut header = Parser::new(&decoded[..first], 0);
    let mut object_numbers = Vec::with_capacity(count);
    let mut offsets = Vec::with_capacity(count);
    for _ in 0..count {
        header.skip_space_comments();
        let number = header
            .atom()
            .ok()
            .and_then(|token| std::str::from_utf8(&token).ok()?.parse::<u32>().ok())
            .filter(|number| *number > 0 && *number as usize <= MAX_OBJECTS)
            .ok_or_else(|| refuse("object stream header has an invalid object number"))?;
        header.skip_space_comments();
        let offset = header
            .atom()
            .ok()
            .and_then(|token| std::str::from_utf8(&token).ok()?.parse::<usize>().ok())
            .filter(|offset| *offset <= u32::MAX as usize)
            .ok_or_else(|| refuse("object stream header has an invalid member offset"))?;
        if offsets.last().is_some_and(|previous| offset <= *previous) {
            return Err(refuse(
                "object stream member offsets are not strictly increasing",
            ));
        }
        object_numbers.push(number);
        offsets.push(offset);
    }
    header.skip_space_comments();
    if header.pos != first {
        return Err(refuse("object stream header does not end at /First"));
    }
    if offsets.first() != Some(&0) {
        return Err(refuse(
            "first object-stream member does not start at /First",
        ));
    }

    let mut parsed_value_nodes = 0usize;
    for index in 0..count {
        let start = first
            .checked_add(offsets[index])
            .filter(|start| *start < decoded.len())
            .ok_or_else(|| refuse("object stream member offset exceeds decoded bytes"))?;
        let end = if index + 1 < count {
            first
                .checked_add(offsets[index + 1])
                .filter(|end| *end <= decoded.len())
                .ok_or_else(|| refuse("next object-stream offset exceeds decoded bytes"))?
        } else {
            decoded.len()
        };
        if end <= start {
            return Err(refuse("object stream has an empty member range"));
        }
        let mut member = Parser::with_value_node_limit(
            &decoded[start..end],
            0,
            value_node_limit.saturating_sub(parsed_value_nodes),
        );
        member.parse_value(0)?;
        member.skip_space_comments();
        if member.pos != end - start {
            return Err(refuse(
                "object stream member contains trailing non-whitespace data",
            ));
        }
        parsed_value_nodes = parsed_value_nodes.saturating_add(member.value_nodes);
    }
    Ok((object_numbers, parsed_value_nodes))
}

fn name_is(value: Option<&Value>, name: &[u8]) -> bool {
    matches!(value, Some(Value::Name(actual)) if actual.as_slice() == name)
}

fn parse(data: &[u8]) -> Result<PdfDoc> {
    let limit = data.len().min(MAX_HEADER_BYTES);
    let header_start = data[..limit]
        .windows(5)
        .position(|window| window == b"%PDF-")
        .ok_or_else(|| Error::parse("pdf: header not found within the first 1024 bytes"))?;
    let line_end = data[header_start..]
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .map(|offset| header_start + offset)
        .unwrap_or(data.len());
    let header = &data[header_start..line_end];
    if header.len() != 8
        || !header.starts_with(b"%PDF-")
        || !header[5].is_ascii_digit()
        || header[6] != b'.'
        || !header[7].is_ascii_digit()
    {
        return Err(Error::parse("pdf: malformed version header"));
    }
    let mut header_end = line_end;
    if data.get(header_end) == Some(&b'\r') {
        header_end += 1;
        if data.get(header_end) == Some(&b'\n') {
            header_end += 1;
        }
    } else if data.get(header_end) == Some(&b'\n') {
        header_end += 1;
    }

    let startxref_start = data
        .windows(b"startxref".len())
        .rposition(|window| window == b"startxref")
        .ok_or_else(|| Error::parse("pdf: missing startxref"))?;
    let mut tail = Parser::new(data, startxref_start);
    tail.expect_keyword(b"startxref", "startxref")?;
    while tail.pos < data.len() && is_white(data[tail.pos]) {
        tail.pos += 1;
    }
    let digits_start = tail.pos;
    let offset_token = tail.atom()?;
    let xref_offset = std::str::from_utf8(&offset_token)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| Error::parse("pdf: invalid startxref offset"))?;
    let digits_end = tail.pos;
    while tail.pos < data.len() && is_white(data[tail.pos]) {
        tail.pos += 1;
    }
    let eof_start = tail.pos;
    if !tail.starts(PDF_EOF) {
        return Err(Error::parse("pdf: startxref is not followed by %%EOF"));
    }
    tail.pos += PDF_EOF.len();
    let eof_count = data[eof_start..]
        .windows(PDF_EOF.len())
        .filter(|window| *window == PDF_EOF)
        .count();
    if eof_count != 1 {
        return Err(refuse(
            "the trailer area does not contain exactly one %%EOF",
        ));
    }
    if data[tail.pos..].iter().any(|byte| !is_white(*byte)) {
        return Err(refuse("non-whitespace bytes follow %%EOF"));
    }

    let is_classic = data.get(xref_offset..xref_offset.saturating_add(4)) == Some(b"xref");
    let (
        xref_sections,
        trailer_value,
        trailer_start,
        trailer_end,
        xref_object,
        xref_stream,
        mut value_nodes,
    ) = if is_classic {
        let (sections, trailer_start, trailer_value, trailer_end, parsed_nodes) =
            parse_xref(data, xref_offset, MAX_PDF_VALUE_NODES)?;
        if dict_entry(&trailer_value, b"XRefStm").is_some() {
            return Err(refuse("hybrid-reference files containing /XRefStm"));
        }
        (
            sections,
            trailer_value,
            trailer_start,
            trailer_end,
            None,
            None,
            parsed_nodes,
        )
    } else {
        let (object, sections, stream_info, parsed_nodes) =
            parse_xref_stream(data, xref_offset, MAX_PDF_VALUE_NODES)?;
        let end = object.end;
        (
            sections,
            object.value.clone(),
            xref_offset,
            end,
            Some(object),
            Some(stream_info),
            parsed_nodes,
        )
    };
    if contains_key(&trailer_value, b"Prev") {
        return Err(refuse("incremental-update trailer /Prev"));
    }
    if dict_entry(&trailer_value, b"Encrypt").is_some() {
        return Err(refuse("encrypted PDFs"));
    }
    if contains_key(&trailer_value, b"ByteRange") {
        return Err(refuse("signed PDFs containing /ByteRange"));
    }
    if contains_key(&trailer_value, b"Linearized") {
        return Err(refuse("linearized PDF dictionary"));
    }
    let root = dict_entry(&trailer_value, b"Root")
        .and_then(|entry| match &entry.value {
            Value::Ref(id) => Some(id),
            _ => None,
        })
        .ok_or_else(|| refuse("trailer /Root is not a direct indirect reference"))?;
    let info_ref = dict_entry(&trailer_value, b"Info").and_then(|entry| match &entry.value {
        Value::Ref(id) => Some(id),
        _ => None,
    });
    if dict_entry(&trailer_value, b"Info").is_some() && info_ref.is_none() {
        return Err(refuse("trailer /Info is not an indirect reference"));
    }
    let size = number_entry(&trailer_value, b"Size")
        .ok_or_else(|| Error::parse("pdf: trailer /Size is missing or invalid"))?;
    if size == 0 || size > MAX_OBJECTS as u64 + 1 {
        return Err(refuse("trailer /Size exceeds the 100,000-object limit"));
    }

    let xref_object_id = xref_object.as_ref().map(|object| &object.id);
    let mut active = Vec::new();
    let mut object_numbers = HashSet::new();
    let mut maximum_object = 0u32;
    let mut zero_entry = false;
    let mut free_entries = HashMap::new();
    let mut compressed_entries = Vec::new();
    let mut xref_object_entry = false;
    for section in &xref_sections {
        for entry in &section.entries {
            maximum_object = maximum_object.max(entry.id.number);
            if u64::from(entry.id.number) >= size {
                return Err(Error::parse(
                    "pdf: xref object number is outside trailer /Size",
                ));
            }
            match entry.kind {
                0 => {
                    if entry.id.number == 0 {
                        if entry.id.generation != u16::MAX {
                            return Err(Error::parse(
                                "pdf: xref object zero does not have generation 65535",
                            ));
                        }
                        zero_entry = true;
                    }
                    free_entries.insert(entry.id.number, entry.field1);
                }
                1 => {
                    if !object_numbers.insert(entry.id.number) {
                        return Err(refuse("duplicate in-use object number"));
                    }
                    let offset = usize::try_from(entry.field1)
                        .map_err(|_| refuse("xref offset does not fit this platform"))?;
                    if let Some(xref_id) = xref_object_id {
                        if entry.id == *xref_id {
                            if offset != xref_offset {
                                return Err(refuse(
                                    "xref stream's own entry does not point to its object header",
                                ));
                            }
                            xref_object_entry = true;
                            continue;
                        }
                    }
                    if offset >= xref_offset {
                        return Err(Error::parse(
                            "pdf: in-use xref offset points beyond the xref section",
                        ));
                    }
                    active.push((entry.id.clone(), offset));
                }
                2 => {
                    if is_classic {
                        return Err(refuse(
                            "classic xref table contains a compressed-object entry",
                        ));
                    }
                    if !object_numbers.insert(entry.id.number) {
                        return Err(refuse("duplicate compressed object number"));
                    }
                    compressed_entries.push(entry.clone());
                }
                _ => return Err(refuse("xref entry type is unsupported")),
            }
        }
    }
    if !zero_entry || u64::from(maximum_object) + 1 > size {
        return Err(Error::parse(
            "pdf: trailer /Size or object-zero xref entry is invalid",
        ));
    }
    if xref_object.is_some() && !xref_object_entry {
        return Err(refuse(
            "xref stream object is not indexed as an in-use object",
        ));
    }
    if !object_numbers.contains(&root.number)
        || !xref_sections
            .iter()
            .flat_map(|section| &section.entries)
            .any(|entry| entry.id == *root && entry.kind != 0)
    {
        return Err(refuse("trailer /Root does not resolve to an in-use object"));
    }
    if info_ref.is_some_and(|id| {
        !xref_sections
            .iter()
            .flat_map(|section| &section.entries)
            .any(|entry| entry.id == *id && entry.kind != 0)
    }) {
        return Err(refuse("trailer /Info does not resolve to an in-use object"));
    }
    for (number, next_free) in &free_entries {
        if *next_free != 0 {
            let target = u32::try_from(*next_free).ok();
            if target.is_none_or(|target| target == *number || !free_entries.contains_key(&target))
            {
                return Err(refuse(
                    "xref free-list pointer does not name another free object",
                ));
            }
        }
    }

    let mut length_values = HashMap::new();
    for (expected_id, offset) in &active {
        if let Some((parsed_id, value, _)) = parse_numeric_object(data, *offset)? {
            if parsed_id == *expected_id {
                length_values.insert(expected_id.clone(), value);
            }
        }
    }
    active.sort_by_key(|(_, offset)| *offset);
    let mut objects = Vec::with_capacity(active.len());
    let mut prior_end = header_end;
    for (id, offset) in active {
        if offset < prior_end {
            return Err(Error::parse(
                "pdf: xref object offsets overlap or precede the header",
            ));
        }
        if !valid_gap(data, prior_end..offset) {
            return Err(refuse("non-object data occurs between indexed objects"));
        }
        let remaining_value_nodes = MAX_PDF_VALUE_NODES.saturating_sub(value_nodes);
        let (object, parsed_nodes) =
            parse_object_with_lengths(data, offset, &length_values, remaining_value_nodes)?;
        value_nodes += parsed_nodes;
        if object.id != id {
            return Err(Error::parse(
                "pdf: xref entry points to a different object number/generation",
            ));
        }
        if contains_key(&object.value, b"ByteRange") {
            return Err(refuse("signed PDF object containing /ByteRange"));
        }
        if contains_key(&object.value, b"Linearized") {
            return Err(refuse("linearized PDFs"));
        }
        if name_is(
            dict_entry(&object.value, b"Type").map(|entry| &entry.value),
            b"Hint",
        ) {
            return Err(refuse("linearization hint stream"));
        }
        if name_is(
            dict_entry(&object.value, b"Type").map(|entry| &entry.value),
            b"XRef",
        ) {
            return Err(refuse("xref stream appears outside startxref"));
        }
        if is_classic
            && name_is(
                dict_entry(&object.value, b"Type").map(|entry| &entry.value),
                b"ObjStm",
            )
        {
            return Err(refuse(
                "object streams require an xref stream in this subset",
            ));
        }
        prior_end = object.end;
        objects.push(object);
    }
    if !valid_gap(data, prior_end..xref_offset) {
        return Err(refuse(
            "non-object data occurs between the last object and xref",
        ));
    }
    if !valid_gap(data, trailer_end..startxref_start) {
        return Err(refuse(
            "unexpected tokens between xref section and startxref",
        ));
    }

    let object_indices: HashMap<u32, usize> = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.id.number, index))
        .collect();
    let mut object_stream_members: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut object_stream_value_nodes = 0usize;
    let mut member_indexes: HashMap<u32, HashSet<usize>> = HashMap::new();
    for entry in &compressed_entries {
        let stream_number = u32::try_from(entry.field1)
            .map_err(|_| refuse("object-stream number does not fit this platform"))?;
        let stream_object = object_indices
            .get(&stream_number)
            .and_then(|index| objects.get(*index))
            .filter(|object| object.id.generation == 0)
            .ok_or_else(|| refuse("compressed object points to a missing object stream"))?;
        if let std::collections::hash_map::Entry::Vacant(entry) =
            object_stream_members.entry(stream_number)
        {
            let node_limit = MAX_PDF_VALUE_NODES
                .saturating_sub(value_nodes.saturating_add(object_stream_value_nodes));
            let (members, parsed_nodes) =
                object_stream_member_ids(data, stream_object, node_limit)?;
            object_stream_value_nodes = object_stream_value_nodes.saturating_add(parsed_nodes);
            entry.insert(members);
        }
        let members = object_stream_members.get(&stream_number).unwrap();
        let index = object_stream_xref_index(entry, members)?;
        if !member_indexes
            .entry(stream_number)
            .or_default()
            .insert(index)
        {
            return Err(refuse(
                "two xref entries point to the same object-stream member",
            ));
        }
    }
    for object in &objects {
        if name_is(
            dict_entry(&object.value, b"Type").map(|entry| &entry.value),
            b"ObjStm",
        ) {
            if object.id.generation != 0 {
                return Err(refuse("object stream generation is not zero"));
            }
            if let std::collections::hash_map::Entry::Vacant(entry) =
                object_stream_members.entry(object.id.number)
            {
                let node_limit = MAX_PDF_VALUE_NODES
                    .saturating_sub(value_nodes.saturating_add(object_stream_value_nodes));
                let (members, parsed_nodes) = object_stream_member_ids(data, object, node_limit)?;
                object_stream_value_nodes = object_stream_value_nodes.saturating_add(parsed_nodes);
                entry.insert(members);
            }
            let count = object_stream_members[&object.id.number].len();
            let indexed = member_indexes.get(&object.id.number);
            if (0..count).any(|index| !indexed.is_some_and(|values| values.contains(&index))) {
                return Err(refuse(
                    "object stream contains a member missing from the xref index",
                ));
            }
        }
    }

    Ok(PdfDoc {
        header_start,
        header_end,
        xref_start: xref_offset,
        xref_sections,
        objects,
        xref_object,
        xref_stream,
        trailer_start,
        startxref_start,
        startxref_digits: digits_start..digits_end,
    })
}

fn write_xref_field(
    output: &mut Vec<u8>,
    value: u64,
    width: usize,
    default: Option<u64>,
) -> Result<()> {
    if width == 0 {
        if default != Some(value) {
            return Err(refuse(
                "xref stream value cannot be represented by a zero-width field",
            ));
        }
        return Ok(());
    }
    if width > 8 || (width < 8 && value >= (1u64 << (width * 8))) {
        return Err(refuse(
            "updated xref value does not fit its declared /W width",
        ));
    }
    for shift in (0..width).rev() {
        output.push((value >> (shift * 8)) as u8);
    }
    Ok(())
}

fn build_xref_stream_data(
    doc: &PdfDoc,
    shifted_offsets: &HashMap<ObjectId, u64>,
    xref_offset: u64,
) -> Result<Vec<u8>> {
    let stream_info = doc
        .xref_stream
        .as_ref()
        .ok_or_else(|| Error::Internal("xref stream metadata is missing".into()))?;
    let row_bytes = stream_info.widths.iter().sum::<usize>();
    let mut output = Vec::with_capacity(
        doc.xref_sections
            .iter()
            .map(|section| section.entries.len())
            .sum::<usize>()
            .saturating_mul(row_bytes),
    );
    for section in &doc.xref_sections {
        for entry in &section.entries {
            let (kind, field1, field2) = match entry.kind {
                0 => (0u64, entry.field1, u64::from(entry.id.generation)),
                1 => {
                    let offset = if entry.id == stream_info.object_id {
                        xref_offset
                    } else {
                        *shifted_offsets.get(&entry.id).ok_or_else(|| {
                            Error::Internal("active PDF xref entry lost its object".into())
                        })?
                    };
                    (1, offset, u64::from(entry.id.generation))
                }
                2 => (2, entry.field1, entry.field2),
                _ => return Err(Error::Internal("unsupported xref entry type".into())),
            };
            write_xref_field(&mut output, kind, stream_info.widths[0], Some(1))?;
            write_xref_field(&mut output, field1, stream_info.widths[1], Some(0))?;
            write_xref_field(&mut output, field2, stream_info.widths[2], Some(0))?;
        }
    }
    Ok(output)
}

fn build_classic_xref(doc: &PdfDoc, shifted_offsets: &HashMap<ObjectId, u64>) -> Result<Vec<u8>> {
    let mut xref = Vec::new();
    xref.extend_from_slice(b"xref\n");
    for section in &doc.xref_sections {
        xref.extend_from_slice(format!("{} {}\n", section.first, section.entries.len()).as_bytes());
        for entry in &section.entries {
            match entry.kind {
                1 => {
                    let offset = shifted_offsets.get(&entry.id).copied().ok_or_else(|| {
                        Error::Internal("active PDF xref entry lost its object".into())
                    })?;
                    if offset > 9_999_999_999 {
                        return Err(refuse(
                            "updated object offset exceeds classic-xref's 10-digit limit",
                        ));
                    }
                    xref.extend_from_slice(
                        format!("{offset:010} {:05} n \n", entry.id.generation).as_bytes(),
                    );
                }
                0 => {
                    if entry.field1 > 9_999_999_999 {
                        return Err(refuse(
                            "xref free-list pointer exceeds the 10-digit table field",
                        ));
                    }
                    xref.extend_from_slice(
                        format!("{:010} {:05} f \n", entry.field1, entry.id.generation).as_bytes(),
                    );
                }
                _ => {
                    return Err(refuse(
                        "classic xref table contains a compressed-object entry",
                    ))
                }
            }
        }
    }
    Ok(xref)
}

fn replace_region(input: &[u8], range: Range<usize>, patches: &[Patch]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut cursor = range.start;
    for patch in patches {
        if patch.range.start < cursor || patch.range.end > range.end {
            return Err(Error::Internal(
                "PDF xref patch lies outside its object".into(),
            ));
        }
        output.extend_from_slice(&input[cursor..patch.range.start]);
        output.extend_from_slice(&patch.bytes);
        cursor = patch.range.end;
    }
    output.extend_from_slice(&input[cursor..range.end]);
    Ok(output)
}

fn plan(input: &[u8], options: &Options) -> Result<Option<RewritePlan>> {
    let doc = parse(input)?;
    let objects_by_id = object_map(&doc);
    let mut patches = Vec::new();
    let mut length_ref_uses: HashMap<ObjectId, usize> = HashMap::new();
    for object in &doc.objects {
        if let Some(length_ref) = object
            .stream
            .as_ref()
            .and_then(|stream| stream.length_ref.as_ref())
        {
            *length_ref_uses.entry(length_ref.clone()).or_default() += 1;
        }
    }
    if let Some(xref_object) = &doc.xref_object {
        if let Some(length_ref) = xref_object
            .stream
            .as_ref()
            .and_then(|stream| stream.length_ref.as_ref())
        {
            *length_ref_uses.entry(length_ref.clone()).or_default() += 1;
        }
    }
    for object in &doc.objects {
        if name_is(
            dict_entry(&object.value, b"Type").map(|entry| &entry.value),
            b"ObjStm",
        ) {
            // Object streams contain objects with no independent byte ranges;
            // leave their complete object bodies byte-for-byte unchanged.
            continue;
        }
        let Some(stream) = &object.stream else {
            continue;
        };
        if !stream.flate_decode {
            continue;
        }
        let payload = &input[stream.data_range.clone()];
        let mut candidate = Vec::new();
        match super::zlib::optimize(payload, &mut candidate, options)? {
            Cand::NoGain => continue,
            Cand::Written(length) => {
                if length as usize != candidate.len() || candidate.len() >= payload.len() {
                    return Err(Error::Internal(
                        "zlib handler returned an inconsistent PDF stream size".into(),
                    ));
                }
                super::zlib::verify(payload, &candidate, options)?;
                if let Some(length_range) = &stream.length_range {
                    patches.push(Patch {
                        range: length_range.clone(),
                        bytes: candidate.len().to_string().into_bytes(),
                    });
                } else if let Some(length_ref) = &stream.length_ref {
                    if length_ref_uses.get(length_ref).copied() != Some(1) {
                        return Err(refuse(
                            "indirect /Length object is shared by multiple streams",
                        ));
                    }
                    let length_object = *objects_by_id
                        .get(length_ref)
                        .ok_or_else(|| refuse("indirect /Length object is not directly indexed"))?;
                    if length_object.stream.is_some()
                        || !matches!(&length_object.value, Value::Number(value) if *value == stream.length as u64)
                    {
                        return Err(refuse("indirect /Length object is not a matching integer"));
                    }
                    patches.push(Patch {
                        range: length_object.value_range.clone(),
                        bytes: candidate.len().to_string().into_bytes(),
                    });
                } else {
                    return Err(Error::Internal(
                        "PDF stream has no usable /Length value".into(),
                    ));
                }
                patches.push(Patch {
                    range: stream.data_range.clone(),
                    bytes: candidate,
                });
            }
        }
    }
    if patches.is_empty() {
        return Ok(None);
    }
    patches.sort_by_key(|patch| patch.range.start);
    for pair in patches.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            return Err(Error::Internal("overlapping PDF rewrite ranges".into()));
        }
    }

    let mut delta_before = 0i128;
    let mut patch_index = 0usize;
    let mut shifted_offsets = HashMap::new();
    for object in &doc.objects {
        while patch_index < patches.len() && patches[patch_index].range.end <= object.start {
            delta_before += patches[patch_index].bytes.len() as i128
                - (patches[patch_index].range.end - patches[patch_index].range.start) as i128;
            patch_index += 1;
        }
        let shifted = object.start as i128 + delta_before;
        let shifted = u64::try_from(shifted)
            .map_err(|_| Error::Internal("PDF xref offset underflow".into()))?;
        if doc.xref_stream.is_none() && shifted > 9_999_999_999 {
            return Err(refuse(
                "updated object offset exceeds classic-xref's 10-digit limit",
            ));
        }
        shifted_offsets.insert(object.id.clone(), shifted);
    }
    let total_delta: i128 = patches
        .iter()
        .map(|patch| patch.bytes.len() as i128 - (patch.range.end - patch.range.start) as i128)
        .sum();
    let xref_offset = u64::try_from(doc.xref_start as i128 + total_delta)
        .map_err(|_| Error::Internal("PDF updated xref offset underflow".into()))?;
    if doc.xref_stream.is_none() && xref_offset > 9_999_999_999 {
        return Err(refuse(
            "updated xref offset exceeds classic-xref's 10-digit limit",
        ));
    }

    let (xref, xref_stream_object, candidate_len) = if let Some(stream_info) = &doc.xref_stream {
        let xref_object = doc
            .xref_object
            .as_ref()
            .ok_or_else(|| Error::Internal("xref stream object is missing".into()))?;
        if xref_object.id != stream_info.object_id {
            return Err(Error::Internal(
                "xref stream object identity changed".into(),
            ));
        }
        let decoded = build_xref_stream_data(&doc, &shifted_offsets, xref_offset)?;
        let encoded = encode_with_codec(&decoded, &stream_info.codec)?;
        let stream = xref_object
            .stream
            .as_ref()
            .ok_or_else(|| Error::Internal("xref stream has no payload".into()))?;
        let xref_patches = vec![
            Patch {
                range: stream
                    .length_range
                    .clone()
                    .ok_or_else(|| refuse("xref stream has an indirect /Length"))?,
                bytes: encoded.len().to_string().into_bytes(),
            },
            Patch {
                range: stream.data_range.clone(),
                bytes: encoded,
            },
        ];
        let xref_object_bytes =
            replace_region(input, xref_object.start..xref_object.end, &xref_patches)?;
        let original_tail_len = input.len().saturating_sub(xref_object.end);
        let old_digits = doc.startxref_digits.end - doc.startxref_digits.start;
        let new_digits = xref_offset.to_string().len();
        let candidate_len = usize::try_from(xref_offset)
            .map_err(|_| Error::Internal("PDF candidate length does not fit this platform".into()))?
            .saturating_add(xref_object_bytes.len())
            .saturating_add(original_tail_len)
            .saturating_sub(old_digits)
            .saturating_add(new_digits);
        (Vec::new(), Some(xref_object_bytes), candidate_len)
    } else {
        let xref = build_classic_xref(&doc, &shifted_offsets)?;
        let prexref_len = usize::try_from(doc.xref_start as i128 + total_delta)
            .map_err(|_| Error::Internal("PDF candidate length underflow".into()))?;
        let trailer_and_tail = input.len().saturating_sub(doc.trailer_start);
        let old_startxref_len = doc.startxref_digits.end - doc.startxref_digits.start;
        let new_startxref_len = xref_offset.to_string().len();
        let candidate_len = prexref_len
            .saturating_add(xref.len())
            .saturating_add(trailer_and_tail)
            .saturating_sub(old_startxref_len)
            .saturating_add(new_startxref_len);
        (xref, None, candidate_len)
    };
    if candidate_len >= input.len() {
        return Ok(None);
    }
    Ok(Some(RewritePlan {
        patches,
        xref,
        xref_stream_object,
        xref_offset,
        candidate_len,
    }))
}

fn write_prefix_with_patches(
    input: &[u8],
    xref_start: usize,
    patches: &[Patch],
    dst: &mut dyn Write,
) -> Result<()> {
    let mut cursor = 0usize;
    for patch in patches {
        if patch.range.start < cursor || patch.range.end > xref_start {
            return Err(Error::Internal(
                "PDF patch lies outside the pre-xref object area".into(),
            ));
        }
        dst.write_all(&input[cursor..patch.range.start])?;
        dst.write_all(&patch.bytes)?;
        cursor = patch.range.end;
    }
    dst.write_all(&input[cursor..xref_start])?;
    Ok(())
}

pub fn optimize(input: &[u8], dst: &mut dyn Write, options: &Options) -> Result<Cand> {
    let Some(plan) = plan(input, options)? else {
        return Ok(Cand::NoGain);
    };
    let doc = parse(input)?;
    write_prefix_with_patches(input, doc.xref_start, &plan.patches, dst)?;
    if let Some(xref_object_bytes) = &plan.xref_stream_object {
        let xref_object = doc
            .xref_object
            .as_ref()
            .ok_or_else(|| Error::Internal("PDF xref stream object is missing".into()))?;
        dst.write_all(xref_object_bytes)?;
        dst.write_all(&input[xref_object.end..doc.startxref_digits.start])?;
    } else {
        dst.write_all(&plan.xref)?;
        dst.write_all(&input[doc.trailer_start..doc.startxref_digits.start])?;
    }
    dst.write_all(plan.xref_offset.to_string().as_bytes())?;
    dst.write_all(&input[doc.startxref_digits.end..])?;
    Ok(Cand::Written(plan.candidate_len as u64))
}

fn object_map(doc: &PdfDoc) -> HashMap<ObjectId, &PdfObject> {
    doc.objects
        .iter()
        .map(|object| (object.id.clone(), object))
        .collect()
}

fn same_gap_sequences(a: &[u8], left: &PdfDoc, b: &[u8], right: &PdfDoc) -> bool {
    if a[..left.header_start] != b[..right.header_start]
        || a[left.header_start..left.header_end] != b[right.header_start..right.header_end]
        || left.objects.len() != right.objects.len()
    {
        return false;
    }
    let mut left_cursor = left.header_end;
    let mut right_cursor = right.header_end;
    for (x, y) in left.objects.iter().zip(&right.objects) {
        if a[left_cursor..x.start] != b[right_cursor..y.start] {
            return false;
        }
        left_cursor = x.end;
        right_cursor = y.end;
    }
    a[left_cursor..left.xref_start] == b[right_cursor..right.xref_start]
}

fn same_object_except_stream(
    a: &[u8],
    left: &PdfObject,
    b: &[u8],
    right: &PdfObject,
    options: &Options,
) -> Result<bool> {
    if left.id != right.id {
        return Ok(false);
    }
    let is_object_stream = name_is(
        dict_entry(&left.value, b"Type").map(|entry| &entry.value),
        b"ObjStm",
    ) || name_is(
        dict_entry(&right.value, b"Type").map(|entry| &entry.value),
        b"ObjStm",
    );
    if is_object_stream {
        return Ok(a[left.start..left.end] == b[right.start..right.end]);
    }
    match (&left.stream, &right.stream) {
        (None, None) => Ok(a[left.start..left.end] == b[right.start..right.end]),
        (Some(x), Some(y)) => {
            let x_bytes = &a[x.data_range.clone()];
            let y_bytes = &b[y.data_range.clone()];
            if x_bytes == y_bytes {
                return Ok(a[left.start..left.end] == b[right.start..right.end]);
            }
            if !x.flate_decode || !y.flate_decode || y_bytes.len() >= x_bytes.len() {
                return Ok(false);
            }
            if x.length_ref != y.length_ref {
                return Ok(false);
            }
            match (&x.length_range, &y.length_range) {
                (Some(x_range), Some(y_range)) => {
                    let x_length = &a[x_range.clone()];
                    let y_length = &b[y_range.clone()];
                    if std::str::from_utf8(y_length)
                        .ok()
                        .and_then(|value| value.parse::<usize>().ok())
                        != Some(y.length)
                        || std::str::from_utf8(x_length)
                            .ok()
                            .and_then(|value| value.parse::<usize>().ok())
                            != Some(x.length)
                    {
                        return Ok(false);
                    }
                    let x_prefix_len = x_range.start - left.start;
                    let y_prefix_len = y_range.start - right.start;
                    if a[left.start..left.start + x_prefix_len]
                        != b[right.start..right.start + y_prefix_len]
                    {
                        return Ok(false);
                    }
                    let x_between = x_range.end..x.data_range.start;
                    let y_between = y_range.end..y.data_range.start;
                    if a[x_between] != b[y_between] {
                        return Ok(false);
                    }
                }
                (None, None) if x.length_ref.is_some() => {
                    if a[left.start..x.data_range.start] != b[right.start..y.data_range.start] {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
            if a[x.data_range.end..left.end] != b[y.data_range.end..right.end] {
                return Ok(false);
            }
            super::zlib::verify(x_bytes, y_bytes, options)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn indirect_length_updates(
    original_bytes: &[u8],
    left: &PdfDoc,
    candidate_bytes: &[u8],
    right: &PdfDoc,
) -> Result<HashMap<ObjectId, (usize, usize)>> {
    let right_map = object_map(right);
    let mut updates = HashMap::new();
    for original in &left.objects {
        let Some(a) = &original.stream else { continue };
        let Some(length_ref) = &a.length_ref else {
            continue;
        };
        let candidate = right_map
            .get(&original.id)
            .ok_or_else(|| Error::verify("PDF stream object disappeared"))?;
        let Some(b) = &candidate.stream else {
            return Err(Error::verify("PDF stream object stopped being a stream"));
        };
        if b.length_ref.as_ref() != Some(length_ref) {
            return Err(Error::verify(
                "PDF indirect stream /Length reference changed",
            ));
        }
        if original_bytes[a.data_range.clone()] != candidate_bytes[b.data_range.clone()] {
            if !a.flate_decode || !b.flate_decode || b.length >= a.length {
                return Err(Error::verify(
                    "PDF indirect /Length changed for an ineligible stream",
                ));
            }
            if let Some((_, existing)) = updates.insert(length_ref.clone(), (a.length, b.length)) {
                if existing != b.length {
                    return Err(Error::verify(
                        "shared indirect /Length does not match every rewritten stream",
                    ));
                }
            }
        }
    }
    Ok(updates)
}

fn same_indirect_length_object(
    original: &[u8],
    left: &PdfObject,
    candidate: &[u8],
    right: &PdfObject,
    expected: (usize, usize),
) -> bool {
    if left.id != right.id || left.stream.is_some() || right.stream.is_some() {
        return false;
    }
    if !matches!(&left.value, Value::Number(value) if *value == expected.0 as u64)
        || !matches!(&right.value, Value::Number(value) if *value == expected.1 as u64)
    {
        return false;
    }
    original[left.start..left.value_range.start] == candidate[right.start..right.value_range.start]
        && original[left.value_range.end..left.end] == candidate[right.value_range.end..right.end]
}

fn verify_xref_semantics(left: &PdfDoc, right: &PdfDoc) -> bool {
    if left.xref_sections.len() != right.xref_sections.len()
        || left.xref_stream.is_some() != right.xref_stream.is_some()
    {
        return false;
    }
    let entries_equal = left
        .xref_sections
        .iter()
        .zip(&right.xref_sections)
        .all(|(a, b)| {
            a.first == b.first
                && a.entries.len() == b.entries.len()
                && a.entries.iter().zip(&b.entries).all(|(x, y)| {
                    x.id == y.id
                        && x.kind == y.kind
                        && match x.kind {
                            0 | 2 => x.field1 == y.field1 && x.field2 == y.field2,
                            1 => x.field2 == y.field2,
                            _ => false,
                        }
                })
        });
    if !entries_equal {
        return false;
    }
    match (&left.xref_stream, &right.xref_stream) {
        (Some(a), Some(b)) => {
            a.object_id == b.object_id
                && a.widths == b.widths
                && a.ranges == b.ranges
                && a.codec == b.codec
        }
        (None, None) => true,
        _ => false,
    }
}

fn same_xref_object_except_payload(
    original: &[u8],
    left: &PdfObject,
    candidate: &[u8],
    right: &PdfObject,
) -> bool {
    if left.id != right.id {
        return false;
    }
    let (Some(a), Some(b)) = (&left.stream, &right.stream) else {
        return false;
    };
    if a.length != a.data_range.end - a.data_range.start
        || b.length != b.data_range.end - b.data_range.start
    {
        return false;
    }
    let (Some(original_range), Some(candidate_range)) = (&a.length_range, &b.length_range) else {
        return false;
    };
    let original_length = std::str::from_utf8(&original[original_range.clone()])
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    let candidate_length = std::str::from_utf8(&candidate[candidate_range.clone()])
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    original_length == Some(a.length)
        && candidate_length == Some(b.length)
        && original[left.start..original_range.start]
            == candidate[right.start..candidate_range.start]
        && original[original_range.end..a.data_range.start]
            == candidate[candidate_range.end..b.data_range.start]
        && original[a.data_range.end..left.end] == candidate[b.data_range.end..right.end]
}

pub fn verify(original: &[u8], candidate: &[u8], options: &Options) -> Result<()> {
    if candidate.len() >= original.len() {
        return Err(Error::verify("PDF candidate is not strictly smaller"));
    }
    let left = parse(original).map_err(|error| Error::verify(format!("original: {error}")))?;
    let right = parse(candidate).map_err(|error| Error::verify(format!("candidate: {error}")))?;
    if left.objects.len() != right.objects.len()
        || !left
            .objects
            .iter()
            .zip(&right.objects)
            .all(|(a, b)| a.id == b.id)
        || !verify_xref_semantics(&left, &right)
        || !same_gap_sequences(original, &left, candidate, &right)
    {
        return Err(Error::verify(
            "PDF object order, xref shape, or inter-object bytes changed",
        ));
    }
    let left_map = object_map(&left);
    let right_map = object_map(&right);
    let length_updates = indirect_length_updates(original, &left, candidate, &right)?;
    for (id, object) in &left_map {
        let other = right_map
            .get(id)
            .ok_or_else(|| Error::verify("PDF object number/generation changed"))?;
        if !same_object_except_stream(original, object, candidate, other, options)? {
            let allowed_length_update = length_updates.get(id).is_some_and(|expected| {
                same_indirect_length_object(original, object, candidate, other, *expected)
            });
            if !allowed_length_update {
                return Err(Error::verify(format!(
                    "PDF object {} {} changed outside an eligible FlateDecode stream",
                    id.number, id.generation
                )));
            }
        }
    }
    match (&left.xref_object, &right.xref_object) {
        (Some(a), Some(b)) => {
            if !same_xref_object_except_payload(original, a, candidate, b) {
                return Err(Error::verify(
                    "PDF xref-stream dictionary changed outside its derived /Length and xref bytes",
                ));
            }
            if original[a.end..left.startxref_start] != candidate[b.end..right.startxref_start] {
                return Err(Error::verify(
                    "PDF bytes between xref stream and startxref changed",
                ));
            }
        }
        (None, None) => {
            if original[left.trailer_start..left.startxref_start]
                != candidate[right.trailer_start..right.startxref_start]
            {
                return Err(Error::verify("PDF classic trailer bytes changed"));
            }
        }
        _ => return Err(Error::verify("PDF xref representation changed")),
    }
    if original[left.startxref_start..left.startxref_digits.start]
        != candidate[right.startxref_start..right.startxref_digits.start]
        || original[left.startxref_digits.end..] != candidate[right.startxref_digits.end..]
    {
        return Err(Error::verify("PDF startxref or EOF bytes changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_enforces_its_configured_value_node_limit() {
        let mut within_limit = Parser::with_value_node_limit(b"[1]", 0, 2);
        assert!(within_limit.parse_value(0).is_ok());
        assert_eq!(within_limit.value_nodes, 2);

        let mut over_limit = Parser::with_value_node_limit(b"[1 2]", 0, 2);
        assert!(over_limit.parse_value(0).is_err());
        assert_eq!(over_limit.value_nodes, 2);
    }

    fn number(n: u32, g: u16, body: &[u8]) -> Vec<u8> {
        [format!("{n} {g} obj\n").as_bytes(), body, b"\nendobj\n"].concat()
    }

    fn fixture() -> Vec<u8> {
        let mut compressed = Vec::new();
        let payload = vec![b'A'; 60_000];
        let mut encoder =
            flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::new(1));
        encoder.write_all(&payload).unwrap();
        encoder.finish().unwrap();
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        offsets.push(0usize);
        offsets.push(out.len());
        out.extend(number(1, 0, b"<< /Type /Catalog /Pages 2 0 R >>"));
        offsets.push(out.len());
        out.extend(number(2, 0, b"<< /Type /Pages /Kids [] /Count 0 >>"));
        offsets.push(out.len());
        let mut body = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            compressed.len()
        )
        .into_bytes();
        body.extend_from_slice(&compressed);
        body.extend_from_slice(b"\nendstream");
        out.extend(number(3, 0, &body));
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        for (index, offset) in offsets.iter().enumerate() {
            if index == 0 {
                out.extend_from_slice(b"0000000000 65535 f \n");
            } else {
                out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
            }
        }
        out.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n");
        out.extend_from_slice(xref.to_string().as_bytes());
        out.extend_from_slice(b"\n%%EOF\n");
        out
    }

    fn indirect_length_fixture() -> Vec<u8> {
        let payload = vec![b'A'; 60_000];
        let mut compressed = Vec::new();
        let mut encoder = ZlibEncoder::new(&mut compressed, Compression::new(1));
        encoder.write_all(&payload).unwrap();
        encoder.finish().unwrap();
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = [0usize; 5];
        offsets[1] = out.len();
        out.extend(number(1, 0, b"<< /Type /Catalog /Pages 2 0 R >>"));
        offsets[2] = out.len();
        out.extend(number(2, 0, b"<< /Type /Pages /Kids [] /Count 0 >>"));
        offsets[3] = out.len();
        let mut body = b"<< /Length 4 0 R /Filter /FlateDecode >>\nstream\n".to_vec();
        body.extend_from_slice(&compressed);
        body.extend_from_slice(b"\nendstream");
        out.extend(number(3, 0, &body));
        offsets[4] = out.len();
        out.extend(number(4, 0, compressed.len().to_string().as_bytes()));
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
        out.extend_from_slice(xref.to_string().as_bytes());
        out.extend_from_slice(b"\n%%EOF\n");
        out
    }

    fn xref_stream_fixture(with_object_stream: bool, png_predictor: bool) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let size = if with_object_stream { 6 } else { 5 };
        let mut offsets = vec![None; size];

        if with_object_stream {
            let first_body = b"<< /Type /Catalog /Pages 2 0 R >>";
            let second_body = b"<< /Type /Pages /Kids [] /Count 0 >>";
            let second_offset = first_body.len() + 1;
            let header = format!("1 0 2 {second_offset} ").into_bytes();
            let first = header.len();
            let mut object_bytes = header;
            object_bytes.extend_from_slice(first_body);
            object_bytes.push(b' ');
            object_bytes.extend_from_slice(second_body);
            let mut compressed = Vec::new();
            let mut encoder = ZlibEncoder::new(&mut compressed, Compression::new(1));
            encoder.write_all(&object_bytes).unwrap();
            encoder.finish().unwrap();
            let mut body = format!(
                "<< /Type /ObjStm /N 2 /First {first} /Length {} /Filter /FlateDecode >>\nstream\n",
                compressed.len()
            )
            .into_bytes();
            body.extend_from_slice(&compressed);
            body.extend_from_slice(b"\nendstream");
            offsets[3] = Some(out.len());
            out.extend(number(3, 0, &body));
        } else {
            offsets[1] = Some(out.len());
            out.extend(number(1, 0, b"<< /Type /Catalog /Pages 2 0 R >>"));
            offsets[2] = Some(out.len());
            out.extend(number(2, 0, b"<< /Type /Pages /Kids [] /Count 0 >>"));
        }

        let payload = vec![b'A'; 60_000];
        let mut compressed = Vec::new();
        let mut encoder = ZlibEncoder::new(&mut compressed, Compression::new(1));
        encoder.write_all(&payload).unwrap();
        encoder.finish().unwrap();
        let stream_number = if with_object_stream { 4 } else { 3 };
        let mut stream_body = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            compressed.len()
        )
        .into_bytes();
        stream_body.extend_from_slice(&compressed);
        stream_body.extend_from_slice(b"\nendstream");
        offsets[stream_number] = Some(out.len());
        out.extend(number(stream_number as u32, 0, &stream_body));

        let xref_number = size - 1;
        let xref_offset = out.len();
        offsets[xref_number] = Some(xref_offset);
        let mut rows = Vec::with_capacity(size * 7);
        for (object_number, offset) in offsets.iter().enumerate() {
            if object_number == 0 {
                rows.push(0);
                rows.extend_from_slice(&0u32.to_be_bytes());
                rows.extend_from_slice(&u16::MAX.to_be_bytes());
            } else if with_object_stream && object_number == 1 {
                rows.push(2);
                rows.extend_from_slice(&3u32.to_be_bytes());
                rows.extend_from_slice(&0u16.to_be_bytes());
            } else if with_object_stream && object_number == 2 {
                rows.push(2);
                rows.extend_from_slice(&3u32.to_be_bytes());
                rows.extend_from_slice(&1u16.to_be_bytes());
            } else {
                rows.push(1);
                rows.extend_from_slice(&((*offset).unwrap() as u32).to_be_bytes());
                rows.extend_from_slice(&0u16.to_be_bytes());
            }
        }
        let encoded_rows = if png_predictor {
            let mut encoded = Vec::with_capacity(rows.len() + size);
            let mut previous = [0u8; 7];
            for row in rows.as_chunks::<7>().0 {
                encoded.push(2); // PNG Up filter for fixed Predictor 12.
                for (index, byte) in row.iter().enumerate() {
                    encoded.push(byte.wrapping_sub(previous[index]));
                }
                previous.copy_from_slice(row);
            }
            encoded
        } else {
            rows
        };
        let mut compressed_xref = Vec::new();
        let mut encoder = ZlibEncoder::new(&mut compressed_xref, Compression::new(1));
        encoder.write_all(&encoded_rows).unwrap();
        encoder.finish().unwrap();
        let decode_parms = if png_predictor {
            " /DecodeParms << /Predictor 12 /Columns 7 >>"
        } else {
            ""
        };
        let mut xref_body = format!(
            "<< /Type /XRef /Size {size} /Root 1 0 R /W [1 4 2] /Filter /FlateDecode{decode_parms} /Length {} >>\nstream\n",
            compressed_xref.len()
        )
        .into_bytes();
        xref_body.extend_from_slice(&compressed_xref);
        xref_body.extend_from_slice(b"\nendstream");
        out.extend(number(xref_number as u32, 0, &xref_body));
        out.extend_from_slice(b"\nstartxref\n");
        out.extend_from_slice(xref_offset.to_string().as_bytes());
        out.extend_from_slice(b"\n%%EOF\n");
        out
    }

    fn options() -> Options {
        Options {
            zopfli: false,
            max_pixels: 1,
            max_uncompressed: 1,
            max_ratio: 1,
            strip_metadata: false,
            nested: false,
            max_depth: 0,
            depth: 0,
            allow: crate::formats::Allow::default(),
            fits_tile: false,
        }
    }

    #[test]
    fn compresses_only_flate_payload_and_keeps_object_identity() {
        let original = fixture();
        let mut candidate = Vec::new();
        let result = optimize(&original, &mut candidate, &options()).unwrap();
        assert!(matches!(result, Cand::Written(_)));
        assert!(candidate.len() < original.len());
        verify(&original, &candidate, &options()).unwrap();
        assert_eq!(
            parse(&original)
                .unwrap()
                .objects
                .iter()
                .map(|o| o.id.clone())
                .collect::<Vec<_>>(),
            parse(&candidate)
                .unwrap()
                .objects
                .iter()
                .map(|o| o.id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn updates_indirect_stream_lengths_only_in_the_length_object() {
        let original = indirect_length_fixture();
        assert!(parse(&original).is_ok());
        let mut candidate = Vec::new();
        assert!(matches!(
            optimize(&original, &mut candidate, &options()).unwrap(),
            Cand::Written(_)
        ));
        verify(&original, &candidate, &options()).unwrap();
        let after = parse(&candidate).unwrap();
        let stream = after
            .objects
            .iter()
            .find_map(|object| object.stream.as_ref())
            .unwrap();
        assert!(stream.length_ref.is_some());
        assert_eq!(
            stream.length,
            stream.data_range.end - stream.data_range.start
        );
    }

    #[test]
    fn compresses_flate_streams_in_a_png_predicted_xref_stream() {
        let original = xref_stream_fixture(false, true);
        let original_doc = parse(&original).unwrap();
        assert!(original_doc.xref_stream.is_some());
        let mut candidate = Vec::new();
        assert!(matches!(
            optimize(&original, &mut candidate, &options()).unwrap(),
            Cand::Written(_)
        ));
        verify(&original, &candidate, &options()).unwrap();
        assert_eq!(
            parse(&candidate).unwrap().xref_stream.unwrap().widths,
            [1, 4, 2]
        );
    }

    #[test]
    fn compressed_xref_indexes_are_checked_against_objstm_member_numbers() {
        let members = [1, 2];
        let valid = XrefEntry {
            id: ObjectId {
                number: 2,
                generation: 0,
            },
            kind: 2,
            field1: 3,
            field2: 1,
        };
        assert_eq!(object_stream_xref_index(&valid, &members).unwrap(), 1);

        let wrong_object = XrefEntry {
            id: ObjectId {
                number: 3,
                generation: 0,
            },
            ..valid.clone()
        };
        assert!(object_stream_xref_index(&wrong_object, &members).is_err());
        let outside = XrefEntry { field2: 2, ..valid };
        assert!(object_stream_xref_index(&outside, &members).is_err());
    }

    #[test]
    fn object_stream_members_share_the_bounded_value_node_budget() {
        let original = xref_stream_fixture(true, true);
        let parsed = parse(&original).unwrap();
        let object_stream = parsed
            .objects
            .iter()
            .find(|object| {
                name_is(
                    dict_entry(&object.value, b"Type").map(|entry| &entry.value),
                    b"ObjStm",
                )
            })
            .unwrap();
        let (members, nodes) =
            object_stream_member_ids(&original, object_stream, MAX_PDF_VALUE_NODES).unwrap();
        assert_eq!(members, vec![1, 2]);
        assert!(nodes > 1);
        assert!(object_stream_member_ids(&original, object_stream, nodes - 1).is_err());
    }

    #[test]
    fn preserves_existing_object_stream_and_its_compressed_members_verbatim() {
        let original = xref_stream_fixture(true, true);
        let before = parse(&original).unwrap();
        let object_stream = before
            .objects
            .iter()
            .find(|object| {
                name_is(
                    dict_entry(&object.value, b"Type").map(|entry| &entry.value),
                    b"ObjStm",
                )
            })
            .unwrap();
        let original_bytes = original[object_stream.start..object_stream.end].to_vec();
        let mut candidate = Vec::new();
        assert!(matches!(
            optimize(&original, &mut candidate, &options()).unwrap(),
            Cand::Written(_)
        ));
        verify(&original, &candidate, &options()).unwrap();
        let after = parse(&candidate).unwrap();
        let object_stream_after = after
            .objects
            .iter()
            .find(|object| {
                name_is(
                    dict_entry(&object.value, b"Type").map(|entry| &entry.value),
                    b"ObjStm",
                )
            })
            .unwrap();
        assert_eq!(
            &candidate[object_stream_after.start..object_stream_after.end],
            original_bytes,
            "existing /ObjStm object bytes and compressed members are preserved verbatim"
        );
        assert!(after
            .xref_sections
            .iter()
            .flat_map(|section| &section.entries)
            .any(|entry| {
                entry.kind == 2 && entry.id.number == 1 && entry.field1 == 3 && entry.field2 == 0
            }));
    }

    #[test]
    fn refuses_incremental_encrypted_signed_linearized_and_malformed_xref_pdfs() {
        let base = fixture();
        assert!(parse(&base).is_ok());
        for bad in [
            base.iter()
                .copied()
                .chain(b"\n%%EOF\n".iter().copied())
                .collect::<Vec<_>>(),
            base.iter()
                .copied()
                .chain(b"\n/Prev 1\n".iter().copied())
                .collect::<Vec<_>>(),
            b"%PDF-1.7\n5 0 obj << /Type /XRef >> endobj\nstartxref\n9\n%%EOF\n".to_vec(),
        ] {
            assert!(parse(&bad).is_err());
        }
        let mut bad = base.clone();
        let insertion = bad
            .windows(8)
            .position(|window| window == b"/Catalog")
            .unwrap();
        bad.splice(insertion..insertion, b"/Linearized 1 ".iter().copied());
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn verifier_rejects_changed_inflated_bytes_and_non_flate_changes() {
        let original = fixture();
        let mut candidate = Vec::new();
        assert!(matches!(
            optimize(&original, &mut candidate, &options()).unwrap(),
            Cand::Written(_)
        ));
        let mut corrupt = candidate.clone();
        let doc = parse(&corrupt).unwrap();
        let stream = doc
            .objects
            .iter()
            .find_map(|object| object.stream.as_ref())
            .unwrap();
        corrupt[stream.data_range.start + 2] ^= 0x10;
        assert!(verify(&original, &corrupt, &options()).is_err());
    }
}
