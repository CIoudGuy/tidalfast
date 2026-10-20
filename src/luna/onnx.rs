//! Reads an ONNX model file (a protocol buffer) into a graph: nodes,
//! constant tensors and the graph's inputs and outputs, for the depth
//! estimation a Luna plugin asks for (`depth.rs`, run by `onnx_run.rs`).
//! Only what running a graph needs is kept; documentation, value infos and
//! training data are skipped. Tensors stored outside the file are refused.

use std::fmt;

/// A model file that is not ONNX, or uses something this reader does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

/// Element data of a tensor.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    F32(Vec<f32>),
    I64(Vec<i64>),
    I32(Vec<i32>),
    U8(Vec<u8>),
    I8(Vec<i8>),
    Bool(Vec<bool>),
}

impl Data {
    pub fn len(&self) -> usize {
        match self {
            Data::F32(v) => v.len(),
            Data::I64(v) => v.len(),
            Data::I32(v) => v.len(),
            Data::U8(v) => v.len(),
            Data::I8(v) => v.len(),
            Data::Bool(v) => v.len(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Data::F32(_) => "float32",
            Data::I64(_) => "int64",
            Data::I32(_) => "int32",
            Data::U8(_) => "uint8",
            Data::I8(_) => "int8",
            Data::Bool(_) => "bool",
        }
    }
}

/// A tensor: its shape (row-major) and elements.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Data,
}

impl Tensor {
    pub fn new(shape: Vec<usize>, data: Data) -> Result<Self> {
        let n: usize = shape.iter().product();
        if n != data.len() {
            return err(format!(
                "a tensor of shape {shape:?} cannot hold {} values",
                data.len()
            ));
        }
        Ok(Self { shape, data })
    }

    pub fn f32(shape: Vec<usize>, v: Vec<f32>) -> Result<Self> {
        Self::new(shape, Data::F32(v))
    }

    pub fn i64(shape: Vec<usize>, v: Vec<i64>) -> Result<Self> {
        Self::new(shape, Data::I64(v))
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// An attribute of a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    Float(f32),
    Int(i64),
    Str(String),
    Tensor(Tensor),
    Floats(Vec<f32>),
    Ints(Vec<i64>),
    /// A kind this reader keeps no value for (graphs, sparse tensors).
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub op: String,
    pub domain: String,
    pub name: String,
    /// Input value names; an empty name is an omitted optional input.
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub attrs: Vec<(String, Attr)>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    pub fn int(&self, name: &str, default: i64) -> i64 {
        match self.attr(name) {
            Some(Attr::Int(i)) => *i,
            _ => default,
        }
    }

    pub fn float(&self, name: &str, default: f32) -> f32 {
        match self.attr(name) {
            Some(Attr::Float(f)) => *f,
            _ => default,
        }
    }

    pub fn ints(&self, name: &str) -> Option<&[i64]> {
        match self.attr(name) {
            Some(Attr::Ints(v)) => Some(v),
            _ => None,
        }
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        match self.attr(name) {
            Some(Attr::Str(s)) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub initializers: Vec<(String, Tensor)>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// Default-domain operator set the model was exported against.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub opset: i64,
    pub graph: Graph,
}

// ------------------------------------------------------------ wire format

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

enum Value<'a> {
    Varint(u64),
    Fixed64,
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn varint(&mut self) -> Result<u64> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let Some(&b) = self.buf.get(self.pos) else {
                return err("the model file ends inside a number");
            };
            self.pos += 1;
            out |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(out);
            }
        }
        err("the model file has a number that is too long")
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let Some(end) = end else {
            return err("the model file ends inside a field");
        };
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// The next field: its number and value, or `None` at the end.
    fn field(&mut self) -> Result<Option<(u32, Value<'a>)>> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let number = (key >> 3) as u32;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => {
                self.take(8)?;
                Value::Fixed64
            }
            2 => {
                let n = usize::try_from(self.varint()?).unwrap_or(usize::MAX);
                Value::Bytes(self.take(n)?)
            }
            5 => Value::Fixed32(u32::from_le_bytes(
                self.take(4)?.try_into().unwrap_or([0; 4]),
            )),
            w => return err(format!("the model file has an unknown field kind {w}")),
        };
        Ok(Some((number, value)))
    }
}

fn string(v: &Value<'_>) -> Result<String> {
    match v {
        Value::Bytes(b) => Ok(String::from_utf8_lossy(b).into_owned()),
        _ => err("the model file has a malformed name"),
    }
}

fn varint_of(v: &Value<'_>) -> Result<u64> {
    match v {
        Value::Varint(x) => Ok(*x),
        _ => err("the model file has a malformed number"),
    }
}

/// Repeated integers, packed or one per field.
fn push_ints(v: &Value<'_>, out: &mut Vec<i64>) -> Result<()> {
    match v {
        Value::Varint(x) => out.push(*x as i64),
        Value::Bytes(b) => {
            let mut r = Reader::new(b);
            while r.pos < b.len() {
                out.push(r.varint()? as i64);
            }
        }
        _ => return err("the model file has malformed integers"),
    }
    Ok(())
}

/// Repeated floats, packed or one per field.
fn push_floats(v: &Value<'_>, out: &mut Vec<f32>) -> Result<()> {
    match v {
        Value::Fixed32(x) => out.push(f32::from_bits(*x)),
        Value::Bytes(b) => {
            if b.len() % 4 != 0 {
                return err("the model file has malformed floats");
            }
            out.extend(b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
        }
        _ => return err("the model file has malformed floats"),
    }
    Ok(())
}

// ------------------------------------------------------------ messages

const FLOAT: u64 = 1;
const UINT8: u64 = 2;
const INT8: u64 = 3;
const INT32: u64 = 6;
const INT64: u64 = 7;
const BOOL: u64 = 9;

fn tensor(buf: &[u8]) -> Result<(String, Tensor)> {
    let mut r = Reader::new(buf);
    let mut dims = Vec::new();
    let mut kind = 0u64;
    let mut name = String::new();
    let mut raw: Option<&[u8]> = None;
    let mut floats = Vec::new();
    let mut ints = Vec::new();
    let mut ints32 = Vec::new();
    let mut external = false;
    while let Some((n, v)) = r.field()? {
        match n {
            1 => push_ints(&v, &mut dims)?,
            2 => kind = varint_of(&v)?,
            4 => push_floats(&v, &mut floats)?,
            5 => push_ints(&v, &mut ints32)?,
            7 => push_ints(&v, &mut ints)?,
            8 => name = string(&v)?,
            9 => match v {
                Value::Bytes(b) => raw = Some(b),
                _ => return err("the model file has malformed tensor data"),
            },
            13 => external = true,
            14 => external |= varint_of(&v)? == 1,
            _ => {}
        }
    }
    if external {
        return err(format!(
            "tensor {name} is stored outside the model file, which is not supported"
        ));
    }
    let shape: Vec<usize> = dims
        .iter()
        .map(|&d| {
            usize::try_from(d).map_err(|_| Error(format!("tensor {name} has a negative size")))
        })
        .collect::<Result<_>>()?;
    let count = shape
        .iter()
        .try_fold(1usize, |a, &d| a.checked_mul(d))
        .ok_or_else(|| Error(format!("tensor {name} is too large")))?;
    let data = match (kind, raw) {
        (FLOAT, Some(b)) => Data::F32(
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
        ),
        (FLOAT, None) => Data::F32(floats),
        (INT64, Some(b)) => Data::I64(
            b.as_chunks::<8>()
                .0
                .iter()
                .map(|c| i64::from_le_bytes(*c))
                .collect(),
        ),
        (INT64, None) => Data::I64(ints),
        (INT32, Some(b)) => Data::I32(
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|c| i32::from_le_bytes(*c))
                .collect(),
        ),
        (INT32, None) => Data::I32(ints32.iter().map(|&x| x as i32).collect()),
        (UINT8, Some(b)) => Data::U8(b.to_vec()),
        (UINT8, None) => Data::U8(ints32.iter().map(|&x| x as u8).collect()),
        (INT8, Some(b)) => Data::I8(b.iter().map(|&x| x as i8).collect()),
        (INT8, None) => Data::I8(ints32.iter().map(|&x| x as i8).collect()),
        (BOOL, Some(b)) => Data::Bool(b.iter().map(|&x| x != 0).collect()),
        (BOOL, None) => Data::Bool(ints32.iter().map(|&x| x != 0).collect()),
        (k, _) => {
            return err(format!(
                "tensor {name} has element type {k}, which is not supported"
            ));
        }
    };
    if data.len() != count {
        return err(format!(
            "tensor {name} has {} values for shape {shape:?}",
            data.len()
        ));
    }
    Ok((name, Tensor { shape, data }))
}

fn attribute(buf: &[u8]) -> Result<(String, Attr)> {
    let mut r = Reader::new(buf);
    let mut name = String::new();
    let mut f = None;
    let mut i = None;
    let mut s = None;
    let mut t = None;
    let mut floats = Vec::new();
    let mut ints = Vec::new();
    let mut kind = 0u64;
    while let Some((n, v)) = r.field()? {
        match n {
            1 => name = string(&v)?,
            2 => match v {
                Value::Fixed32(x) => f = Some(f32::from_bits(x)),
                _ => return err("the model file has a malformed attribute"),
            },
            3 => i = Some(varint_of(&v)? as i64),
            4 => s = Some(string(&v)?),
            5 => match v {
                Value::Bytes(b) => t = Some(tensor(b)?.1),
                _ => return err("the model file has a malformed attribute"),
            },
            7 => push_floats(&v, &mut floats)?,
            8 => push_ints(&v, &mut ints)?,
            20 => kind = varint_of(&v)?,
            _ => {}
        }
    }
    // AttributeProto.AttributeType: FLOAT 1, INT 2, STRING 3, TENSOR 4,
    // FLOATS 6, INTS 7.
    let attr = match kind {
        1 => Attr::Float(f.unwrap_or(0.0)),
        2 => Attr::Int(i.unwrap_or(0)),
        3 => Attr::Str(s.unwrap_or_default()),
        4 => t.map_or(Attr::Other, Attr::Tensor),
        6 => Attr::Floats(floats),
        7 => Attr::Ints(ints),
        _ => Attr::Other,
    };
    Ok((name, attr))
}

fn node(buf: &[u8]) -> Result<Node> {
    let mut r = Reader::new(buf);
    let mut out = Node {
        op: String::new(),
        domain: String::new(),
        name: String::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        attrs: Vec::new(),
    };
    while let Some((n, v)) = r.field()? {
        match n {
            1 => out.inputs.push(string(&v)?),
            2 => out.outputs.push(string(&v)?),
            3 => out.name = string(&v)?,
            4 => out.op = string(&v)?,
            5 => match v {
                Value::Bytes(b) => out.attrs.push(attribute(b)?),
                _ => return err("the model file has a malformed node"),
            },
            7 => out.domain = string(&v)?,
            _ => {}
        }
    }
    Ok(out)
}

fn value_name(buf: &[u8]) -> Result<String> {
    let mut r = Reader::new(buf);
    while let Some((n, v)) = r.field()? {
        if n == 1 {
            return string(&v);
        }
    }
    Ok(String::new())
}

fn graph(buf: &[u8]) -> Result<Graph> {
    let mut r = Reader::new(buf);
    let mut g = Graph::default();
    while let Some((n, v)) = r.field()? {
        let Value::Bytes(b) = v else { continue };
        match n {
            1 => g.nodes.push(node(b)?),
            5 => g.initializers.push(tensor(b)?),
            11 => g.inputs.push(value_name(b)?),
            12 => g.outputs.push(value_name(b)?),
            _ => {}
        }
    }
    // Inputs that are also initializers are constants with a default
    // (older exports list them): they are not fed.
    let constant: std::collections::HashSet<&str> =
        g.initializers.iter().map(|(n, _)| n.as_str()).collect();
    let inputs = std::mem::take(&mut g.inputs);
    g.inputs = inputs
        .into_iter()
        .filter(|n| !constant.contains(n.as_str()))
        .collect();
    Ok(g)
}

/// Reads an ONNX model.
pub fn parse(buf: &[u8]) -> Result<Model> {
    let mut r = Reader::new(buf);
    let mut opset = None;
    let mut g = None;
    while let Some((n, v)) = r.field()? {
        let Value::Bytes(b) = v else { continue };
        match n {
            7 => g = Some(graph(b)?),
            8 => {
                let mut o = Reader::new(b);
                let mut domain = String::new();
                let mut version = 0i64;
                while let Some((n, v)) = o.field()? {
                    match n {
                        1 => domain = string(&v)?,
                        2 => version = varint_of(&v)? as i64,
                        _ => {}
                    }
                }
                if domain.is_empty() || domain == "ai.onnx" {
                    opset = Some(version);
                }
            }
            _ => {}
        }
    }
    let Some(graph) = g else {
        return err("this is not an ONNX model (it has no graph)");
    };
    Ok(Model {
        opset: opset.unwrap_or(1),
        graph,
    })
}
