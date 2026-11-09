//! A parsed JSON document kept as one flat, immutable block: a tape.
//!
//! `json.parse` writes a document as nodes in document order, 16 bytes
//! each, and one text buffer that holds every string. A `json.Value` that
//! lies in a tape is a [`JsonNode`]: the tape, shared by count, and a node
//! index. Dropping or copying a document is one count change, and
//! `json.dumps` writes a document by scanning its nodes.
//!
//! A tape never changes. Reading a string, list or dict payload of a tape
//! value gives a fresh value whose children are tape values again (see
//! [`JsonNode::payload`]), so a change copies only the level it writes.

use std::sync::{Arc, OnceLock};

use crate::collection_index::DictEntries;
use crate::integer::IntegerValue;
use crate::json_codec::{self, BoundedOutput, JsonNumber};
use crate::runtime_value::{
    EnumVariantValue, MapValue, RuntimeName, SharedType, Value, VariantPayloads, VecValue,
};
use crate::text::Text;

/// What a node holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JsonTag {
    Null = 0,
    False = 1,
    True = 2,
    Int = 3,
    Float = 4,
    String = 5,
    Array = 6,
    Object = 7,
}

impl JsonTag {
    /// The name of the `json.Value` variant a node of this tag is.
    pub fn variant_name(self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::False | Self::True => "Bool",
            Self::Int => "Int",
            Self::Float => "Float",
            Self::String => "String",
            Self::Array => "Array",
            Self::Object => "Object",
        }
    }
}

/// One node. A string keeps its length in `a` and its offset in the text
/// in `b`. An array keeps its element count in `a`, an object its entry
/// count, and both keep in `b` the index of the node just past them, so a
/// reader skips a container in one step. An object's entries follow it as
/// key, value, key, value; a key is a string node. A number keeps its bits
/// in `b`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Node {
    tag: JsonTag,
    a: u32,
    b: u64,
}

const _: () = assert!(std::mem::size_of::<Node>() == 16);

/// Where a node keeps its tag, which generated code reads to test a
/// variant: a [`JsonTag`] as one byte.
pub const NODE_TAG_OFFSET: i32 = std::mem::offset_of!(Node, tag) as i32;
/// Where a node keeps an `Int`'s or a `Float`'s bits.
pub const NODE_SCALAR_OFFSET: i32 = std::mem::offset_of!(Node, b) as i32;

/// The most nodes a thread keeps buffer room for between parses.
const SCRATCH_NODES_KEPT: usize = 1 << 16;

/// A parse's node buffer and its open objects' keys (see [`TapeParser`]).
type ScratchBuffers = (Vec<Node>, Vec<(u64, u32)>);

thread_local! {
    static SCRATCH: std::cell::Cell<ScratchBuffers> =
        const { std::cell::Cell::new((Vec::new(), Vec::new())) };
}

/// A parsed document.
pub struct JsonTape {
    nodes: Box<[Node]>,
    /// The source, then the text of each string that held an escape,
    /// unescaped.
    text: Box<str>,
}

impl std::fmt::Debug for JsonTape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonTape")
            .field("nodes", &self.nodes.len())
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

impl JsonTape {
    /// Parses `source`, or gives `None` for any source the tape does not
    /// take: one that fails to parse, nests deeper than `max_depth`, holds
    /// more than `max_nodes` values, or repeats a key in an object. The
    /// caller then parses it the way it parses into a tree, which reports
    /// what it reports.
    pub fn parse(source: &str, max_depth: usize, max_nodes: usize) -> Option<Arc<Self>> {
        if json_codec::check_input_len(source.len()).is_err() || source.len() > u32::MAX as usize {
            return None;
        }
        let mut text = String::new();
        text.try_reserve(source.len()).ok()?;
        text.push_str(source);
        // The node and key buffers are kept for the thread's next parse, so
        // a parse grows no buffer and the tape takes its nodes in one
        // allocation of their final size.
        let (nodes, keys) = SCRATCH.with(|scratch| scratch.take());
        let mut parser = TapeParser {
            bytes: source.as_bytes(),
            source,
            index: 0,
            values: 0,
            max_depth,
            max_nodes: max_nodes.min(json_codec::MAX_JSON_VALUE_NODES),
            nodes,
            text,
            keys,
        };
        let parsed = parser.value(0).and_then(|()| {
            parser.skip_whitespace();
            (parser.index == parser.bytes.len() && parser.text.len() <= u32::MAX as usize)
                .then_some(())
        });
        let tape = parsed.map(|()| {
            Arc::new(Self {
                nodes: Box::from(parser.nodes.as_slice()),
                text: parser.text.into_boxed_str(),
            })
        });
        let (mut nodes, mut keys) = (parser.nodes, parser.keys);
        // A buffer a huge document grew is not kept.
        if nodes.capacity() <= SCRATCH_NODES_KEPT {
            nodes.clear();
            keys.clear();
            SCRATCH.with(|scratch| scratch.set((nodes, keys)));
        }
        tape
    }

    /// The number of nodes, keys included.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn node(&self, index: u32) -> Node {
        self.nodes[index as usize]
    }

    fn str_at(&self, node: Node) -> &str {
        let start = node.b as usize;
        &self.text[start..start + node.a as usize]
    }

    /// The node just past the one at `index` and everything it holds.
    fn next(&self, index: u32) -> u32 {
        let node = self.node(index);
        match node.tag {
            JsonTag::Array | JsonTag::Object => node.b as u32,
            _ => index + 1,
        }
    }
}

/// A `json.Value` that lies in a tape.
#[derive(Clone)]
pub struct JsonNode {
    tape: Arc<JsonTape>,
    index: u32,
}

impl std::fmt::Debug for JsonNode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonNode")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl PartialEq for JsonNode {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.tape, &other.tape) && self.index == other.index)
            || self.payload() == other.payload()
    }
}

fn json_value_type() -> SharedType {
    static TYPE: OnceLock<SharedType> = OnceLock::new();
    TYPE.get_or_init(|| SharedType::named("json.Value")).clone()
}

fn str_type() -> SharedType {
    static TYPE: OnceLock<SharedType> = OnceLock::new();
    TYPE.get_or_init(|| SharedType::named("str")).clone()
}

impl JsonNode {
    /// The document's root.
    pub fn root(tape: Arc<JsonTape>) -> Self {
        Self { tape, index: 0 }
    }

    pub fn tape(&self) -> &Arc<JsonTape> {
        &self.tape
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    /// The address of the node, which lies in the tape for as long as the
    /// tape lives.
    pub fn address(&self) -> usize {
        std::ptr::from_ref(&self.tape.nodes[self.index as usize]) as usize
    }

    pub fn tag(&self) -> JsonTag {
        self.tape.node(self.index).tag
    }

    /// The node as a `json.Value`.
    pub fn into_value(self) -> Value {
        let tag = self.tag();
        let payloads = match tag {
            JsonTag::Null => VariantPayloads::Values(Vec::new()),
            _ => VariantPayloads::Json(self),
        };
        Value::EnumVariant(EnumVariantValue {
            enum_name: RuntimeName::from_static("json.Value"),
            variant_name: RuntimeName::from_static(tag.variant_name()),
            payloads,
        })
    }

    fn child(&self, index: u32) -> Value {
        Self {
            tape: Arc::clone(&self.tape),
            index,
        }
        .into_value()
    }

    /// The scalar a node of a scalar tag holds, as the payload it is.
    fn scalar(&self) -> Option<Value> {
        let node = self.tape.node(self.index);
        Some(match node.tag {
            JsonTag::False => Value::Bool(false),
            JsonTag::True => Value::Bool(true),
            JsonTag::Int => Value::Int(IntegerValue::from_i64(node.b as i64)),
            JsonTag::Float => Value::Float(f64::from_bits(node.b)),
            _ => return None,
        })
    }

    /// The bits of an `Int` (`kind` 0), a `Float` (1), or a `Bool` (2) as
    /// `json.as_int` and its kin read them, or `None` for a node of another
    /// tag.
    pub fn scalar_bits(&self, kind: i64) -> Option<i64> {
        let node = self.tape.node(self.index);
        match (kind, node.tag) {
            (0, JsonTag::Int) | (1, JsonTag::Float) => Some(node.b as i64),
            (2, JsonTag::False) => Some(0),
            (2, JsonTag::True) => Some(1),
            _ => None,
        }
    }

    /// The text of a string node.
    pub fn as_str(&self) -> Option<&str> {
        let node = self.tape.node(self.index);
        (node.tag == JsonTag::String).then(|| self.tape.str_at(node))
    }

    /// The variant's one payload, made a value: a scalar, a `str`, or a
    /// `list[json.Value]` or `dict[str, json.Value]` of tape values one
    /// level down. `None` for `Null`, which carries none.
    pub fn payload(&self) -> Option<Value> {
        let node = self.tape.node(self.index);
        Some(match node.tag {
            JsonTag::Null => return None,
            JsonTag::String => Value::String(Text::from(self.tape.str_at(node))),
            JsonTag::Array => {
                let mut elements = Vec::with_capacity(node.a as usize);
                let mut index = self.index + 1;
                for _ in 0..node.a {
                    elements.push(self.child(index));
                    index = self.tape.next(index);
                }
                Value::Vec(VecValue {
                    element_type: json_value_type(),
                    elements: elements.into(),
                    str_view: Default::default(),
                })
            }
            JsonTag::Object => Value::Map(MapValue {
                key_type: str_type(),
                value_type: json_value_type(),
                entries: DictEntries::from_json_object(self.clone()),
            }),
            _ => return self.scalar(),
        })
    }

    /// The entries of an object node as values: each key a `str` and each
    /// value a tape value.
    pub fn object_entries(&self) -> Vec<(Value, Value)> {
        let node = self.tape.node(self.index);
        let mut entries = Vec::with_capacity(node.a as usize);
        if node.tag != JsonTag::Object {
            return entries;
        }
        let mut index = self.index + 1;
        for _ in 0..node.a {
            let key = Value::String(Text::from(self.tape.str_at(self.tape.node(index))));
            entries.push((key, self.child(index + 1)));
            index = self.tape.next(index + 1);
        }
        entries
    }

    /// How many elements an array node holds, or entries an object node.
    pub fn entry_count(&self) -> usize {
        let node = self.tape.node(self.index);
        match node.tag {
            JsonTag::Array | JsonTag::Object => node.a as usize,
            _ => 0,
        }
    }

    /// The value under `key` in an object node, read from the tape.
    pub fn lookup_str(&self, key: &str) -> Option<Value> {
        let node = self.tape.node(self.index);
        if node.tag != JsonTag::Object {
            return None;
        }
        let mut index = self.index + 1;
        for _ in 0..node.a {
            if self.tape.str_at(self.tape.node(index)) == key {
                return Some(self.child(index + 1));
            }
            index = self.tape.next(index + 1);
        }
        None
    }

    /// The number of nodes this value spans, keys included.
    pub fn span(&self) -> usize {
        (self.tape.next(self.index) - self.index) as usize
    }

    /// Writes the value as `json.dumps` writes it: objects with their keys
    /// sorted. `remaining` counts down the values the write may visit, and
    /// `None` reports a value over that budget, a depth past
    /// [`json_codec::MAX_JSON_DEPTH`], or a failed write, which the caller
    /// reports through its general path.
    pub(crate) fn write(
        &self,
        output: &mut BoundedOutput,
        indent: Option<usize>,
        depth: usize,
        remaining: &mut usize,
    ) -> Option<()> {
        write_node(&self.tape, self.index, output, indent, depth, remaining)
    }

    /// The value as a [`json_codec::JsonValue`] tree. `visit` is called
    /// once per value before it is converted, and `enter` once per
    /// container with the container's depth, and the first error either
    /// gives is the result.
    pub fn to_json_value<E>(
        &self,
        depth: usize,
        visit: &mut impl FnMut() -> Result<(), E>,
        enter: &mut impl FnMut(usize) -> Result<usize, E>,
    ) -> Result<json_codec::JsonValue, E> {
        to_json_value(&self.tape, self.index, depth, visit, enter)
    }
}

fn write_node(
    tape: &JsonTape,
    index: u32,
    output: &mut BoundedOutput,
    indent: Option<usize>,
    depth: usize,
    remaining: &mut usize,
) -> Option<()> {
    *remaining = remaining.checked_sub(1)?;
    let node = tape.node(index);
    match node.tag {
        JsonTag::Null => output.push_str("null").ok()?,
        JsonTag::False => output.push_str("false").ok()?,
        JsonTag::True => output.push_str("true").ok()?,
        JsonTag::Int => json_codec::write_int(output, node.b as i64).ok()?,
        JsonTag::Float => json_codec::write_float(output, f64::from_bits(node.b)).ok()?,
        JsonTag::String => json_codec::write_string(output, tape.str_at(node)).ok()?,
        JsonTag::Array => {
            let child_depth = depth + 1;
            if child_depth > json_codec::MAX_JSON_DEPTH {
                return None;
            }
            output.push_str("[").ok()?;
            let mut child = index + 1;
            for position in 0..node.a {
                if position > 0 {
                    output.push_str(",").ok()?;
                }
                json_codec::write_item_prefix(output, indent, child_depth).ok()?;
                write_node(tape, child, output, indent, child_depth, remaining)?;
                child = tape.next(child);
            }
            json_codec::write_container_suffix(output, indent, depth, node.a > 0).ok()?;
            output.push_str("]").ok()?;
        }
        JsonTag::Object => {
            let child_depth = depth + 1;
            if child_depth > json_codec::MAX_JSON_DEPTH {
                return None;
            }
            let separator = if indent.is_some() { ": " } else { ":" };
            let mut sorted: Vec<(&str, u32)> = Vec::with_capacity(node.a as usize);
            let mut key = index + 1;
            for _ in 0..node.a {
                sorted.push((tape.str_at(tape.node(key)), key + 1));
                key = tape.next(key + 1);
            }
            sorted.sort_unstable_by(|left, right| left.0.cmp(right.0));
            output.push_str("{").ok()?;
            for (position, (key, value)) in sorted.iter().enumerate() {
                if position > 0 {
                    output.push_str(",").ok()?;
                }
                json_codec::write_item_prefix(output, indent, child_depth).ok()?;
                json_codec::write_string(output, key).ok()?;
                output.push_str(separator).ok()?;
                write_node(tape, *value, output, indent, child_depth, remaining)?;
            }
            json_codec::write_container_suffix(output, indent, depth, node.a > 0).ok()?;
            output.push_str("}").ok()?;
        }
    }
    Some(())
}

fn to_json_value<E>(
    tape: &JsonTape,
    index: u32,
    depth: usize,
    visit: &mut impl FnMut() -> Result<(), E>,
    enter: &mut impl FnMut(usize) -> Result<usize, E>,
) -> Result<json_codec::JsonValue, E> {
    use json_codec::JsonValue;
    visit()?;
    let node = tape.node(index);
    Ok(match node.tag {
        JsonTag::Null => JsonValue::Null,
        JsonTag::False => JsonValue::Bool(false),
        JsonTag::True => JsonValue::Bool(true),
        JsonTag::Int => JsonValue::Int(node.b as i64),
        JsonTag::Float => JsonValue::Float(f64::from_bits(node.b)),
        JsonTag::String => JsonValue::String(tape.str_at(node).to_owned()),
        JsonTag::Array => {
            let child_depth = enter(depth)?;
            let mut values = Vec::with_capacity(node.a as usize);
            let mut child = index + 1;
            for _ in 0..node.a {
                values.push(to_json_value(tape, child, child_depth, visit, enter)?);
                child = tape.next(child);
            }
            JsonValue::Array(values)
        }
        JsonTag::Object => {
            let child_depth = enter(depth)?;
            let mut entries = Vec::with_capacity(node.a as usize);
            let mut key = index + 1;
            for _ in 0..node.a {
                let value = to_json_value(tape, key + 1, child_depth, visit, enter)?;
                entries.push((tape.str_at(tape.node(key)).to_owned(), value));
                key = tape.next(key + 1);
            }
            JsonValue::Object(entries)
        }
    })
}

/// Writes a tape as [`json_codec`]'s fast parser reads a tree: the same
/// grammar, limits and number classes.
struct TapeParser<'a> {
    bytes: &'a [u8],
    source: &'a str,
    index: usize,
    values: usize,
    max_depth: usize,
    max_nodes: usize,
    nodes: Vec<Node>,
    text: String,
    /// The key nodes of every object still open, innermost last, each
    /// with its [`key_print`].
    keys: Vec<(u64, u32)>,
}

/// A key's length and first bytes in one word, which two equal keys share.
fn key_print(key: &str) -> u64 {
    let bytes = key.as_bytes();
    let mut head = [0u8; 8];
    let take = bytes.len().min(7);
    head[..take].copy_from_slice(&bytes[..take]);
    head[7] = bytes.len() as u8;
    u64::from_le_bytes(head)
}

impl TapeParser<'_> {
    /// The text of the string node at `key`.
    fn str_of(&self, key: u32) -> &str {
        let node = self.nodes[key as usize];
        &self.text[node.b as usize..node.b as usize + node.a as usize]
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.index) {
            self.index += 1;
        }
    }

    fn literal(&mut self, text: &[u8]) -> Option<()> {
        let end = self.index.checked_add(text.len())?;
        (self.bytes.get(self.index..end)? == text).then(|| self.index = end)
    }

    fn push(&mut self, tag: JsonTag, a: u32, b: u64) -> Option<u32> {
        let index = u32::try_from(self.nodes.len()).ok()?;
        self.nodes.try_reserve(1).ok()?;
        self.nodes.push(Node { tag, a, b });
        Some(index)
    }

    fn value(&mut self, depth: usize) -> Option<()> {
        self.values += 1;
        if self.values > self.max_nodes {
            return None;
        }
        self.skip_whitespace();
        match *self.bytes.get(self.index)? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(drop),
            b't' => {
                self.literal(b"true")?;
                self.push(JsonTag::True, 0, 0).map(drop)
            }
            b'f' => {
                self.literal(b"false")?;
                self.push(JsonTag::False, 0, 0).map(drop)
            }
            b'n' => {
                self.literal(b"null")?;
                self.push(JsonTag::Null, 0, 0).map(drop)
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn array(&mut self, depth: usize) -> Option<()> {
        if depth > self.max_depth {
            return None;
        }
        self.index += 1;
        let at = self.push(JsonTag::Array, 0, 0)?;
        self.skip_whitespace();
        let mut count = 0u32;
        if self.bytes.get(self.index) == Some(&b']') {
            self.index += 1;
        } else {
            loop {
                self.value(depth)?;
                count += 1;
                self.skip_whitespace();
                match *self.bytes.get(self.index)? {
                    b',' => self.index += 1,
                    b']' => {
                        self.index += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        let end = self.nodes.len() as u64;
        let node = &mut self.nodes[at as usize];
        node.a = count;
        node.b = end;
        Some(())
    }

    fn object(&mut self, depth: usize) -> Option<()> {
        if depth > self.max_depth {
            return None;
        }
        self.index += 1;
        let at = self.push(JsonTag::Object, 0, 0)?;
        self.skip_whitespace();
        let base = self.keys.len();
        if self.bytes.get(self.index) == Some(&b'}') {
            self.index += 1;
        } else {
            loop {
                self.skip_whitespace();
                if self.bytes.get(self.index) != Some(&b'"') {
                    return None;
                }
                let key = self.string()?;
                self.keys.try_reserve(1).ok()?;
                let print = key_print(self.str_of(key));
                self.keys.push((print, key));
                self.skip_whitespace();
                if self.bytes.get(self.index) != Some(&b':') {
                    return None;
                }
                self.index += 1;
                self.value(depth)?;
                self.skip_whitespace();
                match *self.bytes.get(self.index)? {
                    b',' => self.index += 1,
                    b'}' => {
                        self.index += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        let count = self.keys.len() - base;
        if self.repeats_a_key(base) {
            return None;
        }
        self.keys.truncate(base);
        let end = self.nodes.len() as u64;
        let node = &mut self.nodes[at as usize];
        node.a = count as u32;
        node.b = end;
        Some(())
    }

    /// Whether two of the keys from `base` on are equal. A tree keeps the
    /// first key's place and the last value for a repeated key, so such an
    /// object is left to the tree parse.
    fn repeats_a_key(&self, base: usize) -> bool {
        let keys = &self.keys[base..];
        if keys.len() <= 16 {
            // Keys that differ almost always differ in their prints, so the
            // texts are compared only when two prints are equal.
            return keys.iter().enumerate().any(|(at, &(print, key))| {
                keys[..at].iter().any(|&(earlier_print, earlier)| {
                    earlier_print == print && self.str_of(earlier) == self.str_of(key)
                })
            });
        }
        let mut sorted: Vec<&str> = keys.iter().map(|&(_, key)| self.str_of(key)).collect();
        sorted.sort_unstable();
        sorted.windows(2).any(|pair| pair[0] == pair[1])
    }

    /// Reads the string that opens at `index` and pushes its node: its
    /// text in the source when it holds no escape, or unescaped at the end
    /// of the text.
    fn string(&mut self) -> Option<u32> {
        self.index += 1;
        let start = self.index;
        loop {
            match *self.bytes.get(self.index)? {
                b'"' => {
                    let len = u32::try_from(self.index - start).ok()?;
                    self.index += 1;
                    return self.push(JsonTag::String, len, start as u64);
                }
                b'\\' => break,
                0..=0x1f => return None,
                _ => self.index += 1,
            }
        }
        let offset = self.text.len();
        self.text.try_reserve(self.index - start).ok()?;
        let source = self.source;
        self.text.push_str(&source[start..self.index]);
        loop {
            let run = self.index;
            while let Some(&byte) = self.bytes.get(self.index) {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.index += 1;
            }
            self.text.try_reserve(self.index - run).ok()?;
            self.text.push_str(&source[run..self.index]);
            match *self.bytes.get(self.index)? {
                b'"' => {
                    self.index += 1;
                    let len = u32::try_from(self.text.len() - offset).ok()?;
                    return self.push(JsonTag::String, len, offset as u64);
                }
                b'\\' => {
                    let escaped = match *self.bytes.get(self.index + 1)? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            self.index += 2;
                            let first = self.hex4()?;
                            let code = match first {
                                0xd800..=0xdbff => {
                                    self.literal(b"\\u")?;
                                    let second = self.hex4()?;
                                    if !(0xdc00..=0xdfff).contains(&second) {
                                        return None;
                                    }
                                    0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                                }
                                0xdc00..=0xdfff => return None,
                                code => code,
                            };
                            self.text.try_reserve(4).ok()?;
                            self.text.push(char::from_u32(code)?);
                            continue;
                        }
                        _ => return None,
                    };
                    self.index += 2;
                    self.text.try_reserve(1).ok()?;
                    self.text.push(escaped);
                }
                _ => return None,
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.index..self.index.checked_add(4)?)?;
        let mut code = 0u32;
        for &digit in digits {
            code = code * 16 + char::from(digit).to_digit(16)?;
        }
        self.index += 4;
        Some(code)
    }

    fn number(&mut self) -> Option<()> {
        let start = self.index;
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        match *self.bytes.get(self.index)? {
            b'0' => self.index += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        let mut plain = true;
        if self.bytes.get(self.index) == Some(&b'.') {
            plain = false;
            self.index += 1;
            if !self.bytes.get(self.index)?.is_ascii_digit() {
                return None;
            }
            self.digits();
        }
        if let Some(b'e' | b'E') = self.bytes.get(self.index) {
            plain = false;
            self.index += 1;
            if let Some(b'+' | b'-') = self.bytes.get(self.index) {
                self.index += 1;
            }
            if !self.bytes.get(self.index)?.is_ascii_digit() {
                return None;
            }
            self.digits();
        }
        match json_codec::json_number(&self.source[start..self.index], plain)? {
            JsonNumber::Int(value) => self.push(JsonTag::Int, 0, value as u64),
            JsonNumber::Float(value) => self.push(JsonTag::Float, 0, value.to_bits()),
        }
        .map(drop)
    }

    fn digits(&mut self) {
        while self.bytes.get(self.index).is_some_and(u8::is_ascii_digit) {
            self.index += 1;
        }
    }
}

#[cfg(test)]
#[path = "json_tape_tests.rs"]
mod tests;
