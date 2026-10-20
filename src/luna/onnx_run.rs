//! Runs an ONNX graph on the CPU, one node after another, with the
//! operators a vision model exported from PyTorch uses (and the integer
//! ones `onnxruntime`'s dynamic quantization adds). Matrix products go
//! through `matrixmultiply`'s SGEMM; integer products are exact sums of
//! small integers done in `f32`.

use std::collections::HashMap;
use std::sync::Arc;

use super::onnx::{Attr, Data, Error, Model, Node, Result, Tensor, err};

/// A graph ready to run: its constants, and for each node the values
/// that are not needed after it.
pub struct Plan {
    steps: Vec<Step>,
    consts: HashMap<String, Arc<Tensor>>,
    input: String,
    output: String,
}

struct Step {
    node: Node,
    op: Op,
    /// Values last read by this node: dropped once it ran.
    free: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Sqrt,
    Erf,
    Relu,
    Neg,
    Exp,
    Tanh,
    Sigmoid,
    Equal,
    Where,
    ReduceMean,
    Softmax,
    Transpose,
    Reshape,
    Concat,
    Gather,
    Slice,
    Unsqueeze,
    Squeeze,
    Shape,
    Expand,
    Cast,
    Constant,
    Identity,
    Clip,
    DynamicQuantizeLinear,
    MatMul,
    MatMulInteger,
    Conv,
    ConvInteger,
    ConvTranspose,
    Resize,
}

fn op_of(node: &Node) -> Option<Op> {
    if !(node.domain.is_empty() || node.domain == "ai.onnx") {
        return None;
    }
    Some(match node.op.as_str() {
        "Add" => Op::Add,
        "Sub" => Op::Sub,
        "Mul" => Op::Mul,
        "Div" => Op::Div,
        "Pow" => Op::Pow,
        "Sqrt" => Op::Sqrt,
        "Erf" => Op::Erf,
        "Relu" => Op::Relu,
        "Neg" => Op::Neg,
        "Exp" => Op::Exp,
        "Tanh" => Op::Tanh,
        "Sigmoid" => Op::Sigmoid,
        "Equal" => Op::Equal,
        "Where" => Op::Where,
        "ReduceMean" => Op::ReduceMean,
        "Softmax" => Op::Softmax,
        "Transpose" => Op::Transpose,
        "Reshape" => Op::Reshape,
        "Concat" => Op::Concat,
        "Gather" => Op::Gather,
        "Slice" => Op::Slice,
        "Unsqueeze" => Op::Unsqueeze,
        "Squeeze" => Op::Squeeze,
        "Shape" => Op::Shape,
        "Expand" => Op::Expand,
        "Cast" => Op::Cast,
        "Constant" => Op::Constant,
        "Identity" => Op::Identity,
        "Clip" => Op::Clip,
        "DynamicQuantizeLinear" => Op::DynamicQuantizeLinear,
        "MatMul" => Op::MatMul,
        "MatMulInteger" => Op::MatMulInteger,
        "Conv" => Op::Conv,
        "ConvInteger" => Op::ConvInteger,
        "ConvTranspose" => Op::ConvTranspose,
        "Resize" => Op::Resize,
        _ => return None,
    })
}

impl Plan {
    /// Checks every node can run and works out when values can go.
    pub fn new(model: Model) -> Result<Self> {
        let graph = model.graph;
        let [input] = graph.inputs.as_slice() else {
            return err(format!(
                "the model takes {} inputs; one image input is supported",
                graph.inputs.len()
            ));
        };
        let Some(output) = graph.outputs.first() else {
            return err("the model has no output");
        };
        let unsupported: Vec<String> = graph
            .nodes
            .iter()
            .filter(|n| op_of(n).is_none())
            .map(|n| {
                if n.domain.is_empty() {
                    n.op.clone()
                } else {
                    format!("{}.{}", n.domain, n.op)
                }
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if !unsupported.is_empty() {
            return err(format!(
                "the model uses operators this runner does not have: {}",
                unsupported.join(", ")
            ));
        }
        let mut last = HashMap::new();
        for (i, n) in graph.nodes.iter().enumerate() {
            for name in n.inputs.iter().filter(|s| !s.is_empty()) {
                last.insert(name.clone(), i);
            }
        }
        let consts: HashMap<String, Arc<Tensor>> = graph
            .initializers
            .into_iter()
            .map(|(n, t)| (n, Arc::new(t)))
            .collect();
        let mut steps: Vec<Step> = graph
            .nodes
            .into_iter()
            .map(|node| Step {
                op: op_of(&node).unwrap_or(Op::Identity),
                node,
                free: Vec::new(),
            })
            .collect();
        for (name, i) in last {
            if name != *output && !consts.contains_key(&name) {
                steps[i].free.push(name);
            }
        }
        Ok(Self {
            steps,
            consts,
            input: input.clone(),
            output: output.clone(),
        })
    }

    /// Runs the graph on `input` and returns its first output.
    pub fn run(&self, input: Tensor) -> Result<Tensor> {
        let mut values: HashMap<&str, Arc<Tensor>> = HashMap::new();
        values.insert(self.input.as_str(), Arc::new(input));
        for step in &self.steps {
            let n = &step.node;
            let inputs: Vec<Option<Arc<Tensor>>> = n
                .inputs
                .iter()
                .map(|name| {
                    if name.is_empty() {
                        return Ok(None);
                    }
                    values
                        .get(name.as_str())
                        .or_else(|| self.consts.get(name))
                        .cloned()
                        .map(Some)
                        .ok_or_else(|| {
                            Error(format!("{} reads {name}, which is not computed", n.op))
                        })
                })
                .collect::<Result<_>>()?;
            let outs = run_node(step.op, n, &inputs)
                .map_err(|e| Error(format!("{} ({}): {}", n.op, n.name, e.0)))?;
            drop(inputs);
            for name in &step.free {
                values.remove(name.as_str());
            }
            for (name, t) in n.outputs.iter().zip(outs) {
                if !name.is_empty() {
                    values.insert(name.as_str(), Arc::new(t));
                }
            }
        }
        let out = values
            .remove(self.output.as_str())
            .ok_or_else(|| Error("the model's output was never computed".into()))?;
        Ok(Arc::try_unwrap(out).unwrap_or_else(|a| (*a).clone()))
    }
}

// ------------------------------------------------------------ helpers

fn need(inputs: &[Option<Arc<Tensor>>], i: usize) -> Result<&Tensor> {
    inputs
        .get(i)
        .and_then(|t| t.as_deref())
        .ok_or_else(|| Error(format!("input {i} is missing")))
}

fn opt(inputs: &[Option<Arc<Tensor>>], i: usize) -> Option<&Tensor> {
    inputs.get(i).and_then(|t| t.as_deref())
}

fn f32s(t: &Tensor) -> Result<&[f32]> {
    match &t.data {
        Data::F32(v) => Ok(v),
        d => err(format!("expected float32, got {}", d.kind())),
    }
}

/// Integer values of a shape-like tensor.
fn ints(t: &Tensor) -> Result<Vec<i64>> {
    Ok(match &t.data {
        Data::I64(v) => v.clone(),
        Data::I32(v) => v.iter().map(|&x| i64::from(x)).collect(),
        d => return err(format!("expected integers, got {}", d.kind())),
    })
}

fn axis(a: i64, rank: usize) -> Result<usize> {
    let r = rank as i64;
    let x = if a < 0 { a + r } else { a };
    if x < 0 || x >= r.max(1) {
        return err(format!("axis {a} is out of range for rank {rank}"));
    }
    Ok(x as usize)
}

fn strides(shape: &[usize]) -> Vec<usize> {
    let mut s = vec![1; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        s[i] = s[i + 1] * shape[i + 1];
    }
    s
}

fn broadcast_shape(a: &[usize], b: &[usize]) -> Result<Vec<usize>> {
    let n = a.len().max(b.len());
    let mut out = vec![0; n];
    for i in 0..n {
        let da = if i + a.len() >= n {
            a[i + a.len() - n]
        } else {
            1
        };
        let db = if i + b.len() >= n {
            b[i + b.len() - n]
        } else {
            1
        };
        out[i] = if da == db || db == 1 {
            da
        } else if da == 1 {
            db
        } else {
            return err(format!("shapes {a:?} and {b:?} do not broadcast"));
        };
    }
    Ok(out)
}

/// Strides of `shape` read as `out` (0 along broadcast axes).
fn broadcast_strides(shape: &[usize], out: &[usize]) -> Vec<usize> {
    let own = strides(shape);
    let lead = out.len() - shape.len();
    (0..out.len())
        .map(|i| {
            if i < lead || shape[i - lead] == 1 {
                0
            } else {
                own[i - lead]
            }
        })
        .collect()
}

/// Walks `out` with each input's strides, merged so the innermost run is
/// as long as it can be, calling `f(offsets, n, inner_strides)` per run.
fn walk(out: &[usize], ins: &[Vec<usize>], mut f: impl FnMut(&[usize], usize, &[usize])) {
    let total: usize = out.iter().product();
    if total == 0 {
        return;
    }
    // Merge axes that are contiguous for every input.
    let mut dims: Vec<usize> = Vec::new();
    let mut st: Vec<Vec<usize>> = vec![Vec::new(); ins.len()];
    for (i, &d) in out.iter().enumerate() {
        if d == 1 {
            continue;
        }
        if let Some(&last) = dims.last()
            && ins
                .iter()
                .enumerate()
                .all(|(k, s)| st[k].last().copied() == Some(s[i] * d))
        {
            let _ = last;
            let j = dims.len() - 1;
            dims[j] *= d;
            for (k, s) in ins.iter().enumerate() {
                let l = st[k].len() - 1;
                st[k][l] = s[i];
            }
            continue;
        }
        dims.push(d);
        for (k, s) in ins.iter().enumerate() {
            st[k].push(s[i]);
        }
    }
    if dims.is_empty() {
        dims.push(1);
        for s in &mut st {
            s.push(0);
        }
    }
    let rank = dims.len();
    let inner = dims[rank - 1];
    let inner_st: Vec<usize> = st.iter().map(|s| s[rank - 1]).collect();
    let mut idx = vec![0usize; rank - 1];
    let mut offs = vec![0usize; ins.len()];
    loop {
        f(&offs, inner, &inner_st);
        // Next outer index.
        let mut ax = rank - 1;
        loop {
            if ax == 0 {
                return;
            }
            ax -= 1;
            idx[ax] += 1;
            for (k, o) in offs.iter_mut().enumerate() {
                *o += st[k][ax];
            }
            if idx[ax] < dims[ax] {
                break;
            }
            for (k, o) in offs.iter_mut().enumerate() {
                *o -= st[k][ax] * dims[ax];
            }
            idx[ax] = 0;
        }
    }
}

fn binary<T: Copy, U: Copy + Default>(
    a: &[T],
    ash: &[usize],
    b: &[T],
    bsh: &[usize],
    f: impl Fn(T, T) -> U,
) -> Result<(Vec<usize>, Vec<U>)> {
    let shape = broadcast_shape(ash, bsh)?;
    let sa = broadcast_strides(ash, &shape);
    let sb = broadcast_strides(bsh, &shape);
    let total: usize = shape.iter().product();
    let mut out = Vec::with_capacity(total);
    walk(&shape, &[sa, sb], |offs, n, st| {
        let (oa, ob) = (offs[0], offs[1]);
        match (st[0], st[1]) {
            (1, 1) => out.extend(
                a[oa..oa + n]
                    .iter()
                    .zip(&b[ob..ob + n])
                    .map(|(&x, &y)| f(x, y)),
            ),
            (1, 0) => {
                let y = b[ob];
                out.extend(a[oa..oa + n].iter().map(|&x| f(x, y)));
            }
            (0, 1) => {
                let x = a[oa];
                out.extend(b[ob..ob + n].iter().map(|&y| f(x, y)));
            }
            (s, t) => out.extend((0..n).map(|i| f(a[oa + i * s], b[ob + i * t]))),
        }
    });
    Ok((shape, out))
}

/// `t` broadcast to `shape`.
fn expand_to<T: Copy>(v: &[T], from: &[usize], shape: &[usize]) -> Vec<T> {
    let s = broadcast_strides(from, shape);
    let mut out = Vec::with_capacity(shape.iter().product());
    walk(shape, &[s], |offs, n, st| {
        out.extend((0..n).map(|i| v[offs[0] + i * st[0]]));
    });
    out
}

fn same_kind_err<T>(a: &Tensor, b: &Tensor) -> Result<T> {
    err(format!(
        "cannot combine {} with {}",
        a.data.kind(),
        b.data.kind()
    ))
}

// ------------------------------------------------------------ nodes

fn run_node(op: Op, n: &Node, inputs: &[Option<Arc<Tensor>>]) -> Result<Vec<Tensor>> {
    let one = |t: Tensor| Ok(vec![t]);
    match op {
        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow => {
            let (a, b) = (need(inputs, 0)?, need(inputs, 1)?);
            one(arith(op, a, b)?)
        }
        Op::Sqrt => one(unary(need(inputs, 0)?, f32::sqrt)?),
        Op::Erf => one(unary(need(inputs, 0)?, erf)?),
        Op::Relu => one(unary(need(inputs, 0)?, |x| x.max(0.0))?),
        Op::Neg => one(unary(need(inputs, 0)?, |x| -x)?),
        Op::Exp => one(unary(need(inputs, 0)?, f32::exp)?),
        Op::Tanh => one(unary(need(inputs, 0)?, f32::tanh)?),
        Op::Sigmoid => one(unary(need(inputs, 0)?, |x| 1.0 / (1.0 + (-x).exp()))?),
        Op::Equal => {
            let (a, b) = (need(inputs, 0)?, need(inputs, 1)?);
            let (shape, v) = match (&a.data, &b.data) {
                (Data::I64(x), Data::I64(y)) => binary(x, &a.shape, y, &b.shape, |p, q| p == q)?,
                (Data::F32(x), Data::F32(y)) => binary(x, &a.shape, y, &b.shape, |p, q| p == q)?,
                _ => return same_kind_err(a, b),
            };
            one(Tensor::new(shape, Data::Bool(v))?)
        }
        Op::Where => {
            let (c, x, y) = (need(inputs, 0)?, need(inputs, 1)?, need(inputs, 2)?);
            let Data::Bool(cv) = &c.data else {
                return err("the condition is not bool");
            };
            let shape = broadcast_shape(&broadcast_shape(&c.shape, &x.shape)?, &y.shape)?;
            let cv = expand_to(cv, &c.shape, &shape);
            let data = match (&x.data, &y.data) {
                (Data::F32(p), Data::F32(q)) => {
                    let (p, q) = (
                        expand_to(p, &x.shape, &shape),
                        expand_to(q, &y.shape, &shape),
                    );
                    Data::F32(
                        cv.iter()
                            .zip(p.iter().zip(&q))
                            .map(|(&c, (&p, &q))| if c { p } else { q })
                            .collect(),
                    )
                }
                (Data::I64(p), Data::I64(q)) => {
                    let (p, q) = (
                        expand_to(p, &x.shape, &shape),
                        expand_to(q, &y.shape, &shape),
                    );
                    Data::I64(
                        cv.iter()
                            .zip(p.iter().zip(&q))
                            .map(|(&c, (&p, &q))| if c { p } else { q })
                            .collect(),
                    )
                }
                _ => return same_kind_err(x, y),
            };
            one(Tensor::new(shape, data)?)
        }
        Op::ReduceMean => one(reduce_mean(n, inputs)?),
        Op::Softmax => one(softmax(n, need(inputs, 0)?)?),
        Op::Transpose => {
            let t = need(inputs, 0)?;
            let perm: Vec<usize> = match n.ints("perm") {
                Some(p) => p
                    .iter()
                    .map(|&a| axis(a, t.shape.len()))
                    .collect::<Result<_>>()?,
                None => (0..t.shape.len()).rev().collect(),
            };
            one(transpose(t, &perm)?)
        }
        Op::Reshape => {
            let t = need(inputs, 0)?;
            let want = ints(need(inputs, 1)?)?;
            let allow_zero = n.int("allowzero", 0) != 0;
            one(Tensor::new(
                reshape_dims(&t.shape, &want, allow_zero)?,
                t.data.clone(),
            )?)
        }
        Op::Concat => one(concat(n, inputs)?),
        Op::Gather => one(gather(n, need(inputs, 0)?, need(inputs, 1)?)?),
        Op::Slice => one(slice(inputs)?),
        Op::Unsqueeze => {
            let t = need(inputs, 0)?;
            let axes = match opt(inputs, 1) {
                Some(a) => ints(a)?,
                None => n.ints("axes").map(<[i64]>::to_vec).unwrap_or_default(),
            };
            let rank = t.shape.len() + axes.len();
            let mut put: Vec<usize> = axes.iter().map(|&a| axis(a, rank)).collect::<Result<_>>()?;
            put.sort_unstable();
            let mut shape = Vec::with_capacity(rank);
            let mut src = t.shape.iter();
            for i in 0..rank {
                if put.contains(&i) {
                    shape.push(1);
                } else {
                    shape.push(*src.next().ok_or_else(|| Error("bad axes".into()))?);
                }
            }
            one(Tensor::new(shape, t.data.clone())?)
        }
        Op::Squeeze => {
            let t = need(inputs, 0)?;
            let axes = match opt(inputs, 1) {
                Some(a) => Some(ints(a)?),
                None => n.ints("axes").map(<[i64]>::to_vec),
            };
            let drop: Vec<usize> = match axes {
                Some(a) => a
                    .iter()
                    .map(|&a| axis(a, t.shape.len()))
                    .collect::<Result<_>>()?,
                None => (0..t.shape.len()).filter(|&i| t.shape[i] == 1).collect(),
            };
            let shape = t
                .shape
                .iter()
                .enumerate()
                .filter(|(i, _)| !drop.contains(i))
                .map(|(_, &d)| d)
                .collect();
            one(Tensor::new(shape, t.data.clone())?)
        }
        Op::Shape => {
            let t = need(inputs, 0)?;
            let r = t.shape.len() as i64;
            let clampi = |v: i64| (if v < 0 { v + r } else { v }).clamp(0, r) as usize;
            let start = clampi(n.int("start", 0));
            let end = clampi(n.int("end", r));
            let dims: Vec<i64> = t.shape[start..end.max(start)]
                .iter()
                .map(|&d| d as i64)
                .collect();
            one(Tensor::i64(vec![dims.len()], dims)?)
        }
        Op::Expand => {
            let t = need(inputs, 0)?;
            let want: Vec<usize> = ints(need(inputs, 1)?)?
                .iter()
                .map(|&d| usize::try_from(d).map_err(|_| Error("negative size".into())))
                .collect::<Result<_>>()?;
            let shape = broadcast_shape(&t.shape, &want)?;
            let data = match &t.data {
                Data::F32(v) => Data::F32(expand_to(v, &t.shape, &shape)),
                Data::I64(v) => Data::I64(expand_to(v, &t.shape, &shape)),
                Data::I32(v) => Data::I32(expand_to(v, &t.shape, &shape)),
                Data::U8(v) => Data::U8(expand_to(v, &t.shape, &shape)),
                Data::I8(v) => Data::I8(expand_to(v, &t.shape, &shape)),
                Data::Bool(v) => Data::Bool(expand_to(v, &t.shape, &shape)),
            };
            one(Tensor::new(shape, data)?)
        }
        Op::Cast => one(cast(need(inputs, 0)?, n.int("to", 1))?),
        Op::Constant => match n.attr("value") {
            Some(Attr::Tensor(t)) => one(t.clone()),
            _ => match (
                n.attr("value_float"),
                n.attr("value_int"),
                n.attr("value_ints"),
                n.attr("value_floats"),
            ) {
                (Some(Attr::Float(f)), ..) => one(Tensor::f32(vec![], vec![*f])?),
                (_, Some(Attr::Int(i)), ..) => one(Tensor::i64(vec![], vec![*i])?),
                (_, _, Some(Attr::Ints(v)), _) => one(Tensor::i64(vec![v.len()], v.clone())?),
                (_, _, _, Some(Attr::Floats(v))) => one(Tensor::f32(vec![v.len()], v.clone())?),
                _ => err("a constant without a value"),
            },
        },
        Op::Identity => one(need(inputs, 0)?.clone()),
        Op::Clip => {
            let t = need(inputs, 0)?;
            let lo = opt(inputs, 1).map(|t| f32s(t).map(|v| v[0])).transpose()?;
            let hi = opt(inputs, 2).map(|t| f32s(t).map(|v| v[0])).transpose()?;
            let lo = lo.unwrap_or_else(|| n.float("min", f32::NEG_INFINITY));
            let hi = hi.unwrap_or_else(|| n.float("max", f32::INFINITY));
            one(unary(t, |x| x.max(lo).min(hi))?)
        }
        Op::DynamicQuantizeLinear => dynamic_quantize(need(inputs, 0)?),
        Op::MatMul => one(matmul(need(inputs, 0)?, need(inputs, 1)?)?),
        Op::MatMulInteger => one(matmul_integer(inputs)?),
        Op::Conv => {
            let x = need(inputs, 0)?;
            let w = need(inputs, 1)?;
            let mut out = conv(n, &x.shape, f32s(x)?, &w.shape, f32s(w)?)?;
            if let Some(b) = opt(inputs, 2) {
                add_channel_bias(&mut out, f32s(b)?)?;
            }
            one(out)
        }
        Op::ConvInteger => {
            let x = need(inputs, 0)?;
            let w = need(inputs, 1)?;
            let xz = opt(inputs, 2)
                .map(zero_points)
                .transpose()?
                .unwrap_or_default();
            let wz = opt(inputs, 3)
                .map(zero_points)
                .transpose()?
                .unwrap_or_default();
            let Data::U8(xv) = &x.data else {
                return err("ConvInteger input is not uint8");
            };
            let xz = *xz.first().unwrap_or(&0.0);
            let xf: Vec<f32> = xv.iter().map(|&v| f32::from(v) - xz).collect();
            let wf = dequant_rows(w, &wz)?;
            let out = conv(n, &x.shape, &xf, &w.shape, &wf)?;
            let data = f32s(&out)?.iter().map(|&v| v.round() as i32).collect();
            one(Tensor::new(out.shape, Data::I32(data))?)
        }
        Op::ConvTranspose => one(conv_transpose(n, inputs)?),
        Op::Resize => one(resize(n, inputs)?),
    }
}

fn unary(t: &Tensor, f: impl Fn(f32) -> f32) -> Result<Tensor> {
    let v = f32s(t)?;
    Tensor::f32(t.shape.clone(), v.iter().map(|&x| f(x)).collect())
}

fn arith(op: Op, a: &Tensor, b: &Tensor) -> Result<Tensor> {
    let (shape, data) = match (&a.data, &b.data) {
        (Data::F32(x), Data::F32(y)) => {
            let (s, v) = match op {
                Op::Add => binary(x, &a.shape, y, &b.shape, |p, q| p + q)?,
                Op::Sub => binary(x, &a.shape, y, &b.shape, |p, q| p - q)?,
                Op::Mul => binary(x, &a.shape, y, &b.shape, |p, q| p * q)?,
                Op::Div => binary(x, &a.shape, y, &b.shape, |p, q| p / q)?,
                _ => {
                    // x², x^0.5 and the general case.
                    if y.len() == 1 && y[0] == 2.0 {
                        binary(x, &a.shape, y, &b.shape, |p, _| p * p)?
                    } else {
                        binary(x, &a.shape, y, &b.shape, f32::powf)?
                    }
                }
            };
            (s, Data::F32(v))
        }
        (Data::I64(x), Data::I64(y)) => {
            let (s, v) = match op {
                Op::Add => binary(x, &a.shape, y, &b.shape, i64::wrapping_add)?,
                Op::Sub => binary(x, &a.shape, y, &b.shape, i64::wrapping_sub)?,
                Op::Mul => binary(x, &a.shape, y, &b.shape, i64::wrapping_mul)?,
                Op::Div => {
                    if y.contains(&0) {
                        return err("integer division by zero");
                    }
                    binary(x, &a.shape, y, &b.shape, i64::wrapping_div)?
                }
                _ => binary(x, &a.shape, y, &b.shape, |p, q| {
                    p.wrapping_pow(q.clamp(0, 63) as u32)
                })?,
            };
            (s, Data::I64(v))
        }
        (Data::F32(x), Data::I64(y)) if op == Op::Pow => {
            let y: Vec<f32> = y.iter().map(|&q| q as f32).collect();
            let (s, v) = binary(x, &a.shape, &y, &b.shape, f32::powf)?;
            (s, Data::F32(v))
        }
        _ => return same_kind_err(a, b),
    };
    Tensor::new(shape, data)
}

/// The error function, to about 1.2e-7 (Numerical Recipes' `erfc`).
fn erf(x: f32) -> f32 {
    let z = f64::from(x).abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t
        * (-z * z - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07
                                + t * (-1.135_203_98
                                    + t * (1.488_515_87
                                        + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    let e = 1.0 - r;
    (if x < 0.0 { -e } else { e }) as f32
}

fn reduce_mean(n: &Node, inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let t = need(inputs, 0)?;
    let v = f32s(t)?;
    let rank = t.shape.len();
    let axes: Vec<i64> = match opt(inputs, 1) {
        Some(a) => ints(a)?,
        None => n.ints("axes").map(<[i64]>::to_vec).unwrap_or_default(),
    };
    let mut ax: Vec<usize> = if axes.is_empty() {
        (0..rank).collect()
    } else {
        axes.iter().map(|&a| axis(a, rank)).collect::<Result<_>>()?
    };
    ax.sort_unstable();
    ax.dedup();
    let keep = n.int("keepdims", 1) != 0;
    // Only trailing axes are reduced here (LayerNorm's last axis).
    let first = ax[0];
    if ax.iter().enumerate().any(|(i, &a)| a != first + i) || ax[ax.len() - 1] != rank - 1 {
        return err(format!(
            "ReduceMean over axes {ax:?} of rank {rank} is not supported"
        ));
    }
    let inner: usize = t.shape[first..].iter().product();
    let outer = v.len() / inner.max(1);
    let out: Vec<f32> = (0..outer)
        .map(|o| {
            let row = &v[o * inner..(o + 1) * inner];
            (row.iter().map(|&x| f64::from(x)).sum::<f64>() / inner as f64) as f32
        })
        .collect();
    let mut shape: Vec<usize> = t.shape[..first].to_vec();
    if keep {
        shape.extend(std::iter::repeat_n(1, rank - first));
    }
    Tensor::f32(shape, out)
}

fn softmax(n: &Node, t: &Tensor) -> Result<Tensor> {
    let v = f32s(t)?;
    let rank = t.shape.len();
    let a = axis(n.int("axis", -1), rank)?;
    if a != rank - 1 {
        return err("softmax over an axis other than the last is not supported");
    }
    let inner = t.shape[rank - 1].max(1);
    let mut out = v.to_vec();
    for row in out.chunks_mut(inner) {
        let m = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut s = 0.0f32;
        for x in row.iter_mut() {
            *x = (*x - m).exp();
            s += *x;
        }
        let k = 1.0 / s;
        for x in row.iter_mut() {
            *x *= k;
        }
    }
    Tensor::f32(t.shape.clone(), out)
}

fn permute<T: Copy>(v: &[T], shape: &[usize], perm: &[usize]) -> (Vec<usize>, Vec<T>) {
    let src = strides(shape);
    let out_shape: Vec<usize> = perm.iter().map(|&p| shape[p]).collect();
    let st: Vec<usize> = perm.iter().map(|&p| src[p]).collect();
    let mut out = Vec::with_capacity(v.len());
    walk(&out_shape, &[st], |offs, n, s| {
        let (o, s) = (offs[0], s[0]);
        if s == 1 {
            out.extend_from_slice(&v[o..o + n]);
        } else {
            out.extend((0..n).map(|i| v[o + i * s]));
        }
    });
    (out_shape, out)
}

fn transpose(t: &Tensor, perm: &[usize]) -> Result<Tensor> {
    if perm.len() != t.shape.len() {
        return err("the permutation does not match the rank");
    }
    let (shape, data) = match &t.data {
        Data::F32(v) => {
            let (s, o) = permute(v, &t.shape, perm);
            (s, Data::F32(o))
        }
        Data::I64(v) => {
            let (s, o) = permute(v, &t.shape, perm);
            (s, Data::I64(o))
        }
        Data::U8(v) => {
            let (s, o) = permute(v, &t.shape, perm);
            (s, Data::U8(o))
        }
        d => return err(format!("Transpose of {} is not supported", d.kind())),
    };
    Tensor::new(shape, data)
}

fn reshape_dims(shape: &[usize], want: &[i64], allow_zero: bool) -> Result<Vec<usize>> {
    let total: usize = shape.iter().product();
    let mut out = Vec::with_capacity(want.len());
    let mut infer = None;
    for (i, &d) in want.iter().enumerate() {
        match d {
            -1 => {
                if infer.replace(i).is_some() {
                    return err("two -1 sizes in a reshape");
                }
                out.push(1);
            }
            0 if !allow_zero => out.push(*shape.get(i).ok_or_else(|| Error("bad 0 size".into()))?),
            d if d >= 0 => out.push(d as usize),
            _ => return err("a negative size in a reshape"),
        }
    }
    if let Some(i) = infer {
        let known: usize = out.iter().product();
        if known == 0 || !total.is_multiple_of(known) {
            return err(format!("cannot reshape {shape:?} to {want:?}"));
        }
        out[i] = total / known;
    }
    if out.iter().product::<usize>() != total {
        return err(format!("cannot reshape {shape:?} to {want:?}"));
    }
    Ok(out)
}

fn concat(n: &Node, inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let parts: Vec<&Tensor> = inputs.iter().filter_map(|t| t.as_deref()).collect();
    let Some(first) = parts.first() else {
        return err("nothing to concatenate");
    };
    let rank = first.shape.len();
    let a = axis(n.int("axis", 0), rank)?;
    let outer: usize = first.shape[..a].iter().product();
    let mut shape = first.shape.clone();
    shape[a] = 0;
    for p in &parts {
        if p.shape.len() != rank
            || p.shape[..a] != first.shape[..a]
            || p.shape[a + 1..] != first.shape[a + 1..]
        {
            return err("concatenated tensors differ in shape");
        }
        shape[a] += p.shape[a];
    }
    let inner: usize = first.shape[a + 1..].iter().product();
    macro_rules! join {
        ($variant:ident) => {{
            let mut out = Vec::with_capacity(shape.iter().product());
            for o in 0..outer {
                for p in &parts {
                    let Data::$variant(v) = &p.data else {
                        return same_kind_err(first, p);
                    };
                    let len = p.shape[a] * inner;
                    out.extend_from_slice(&v[o * len..(o + 1) * len]);
                }
            }
            Data::$variant(out)
        }};
    }
    let data = match &first.data {
        Data::F32(_) => join!(F32),
        Data::I64(_) => join!(I64),
        Data::I32(_) => join!(I32),
        Data::U8(_) => join!(U8),
        Data::I8(_) => join!(I8),
        Data::Bool(_) => join!(Bool),
    };
    Tensor::new(shape, data)
}

fn gather(n: &Node, t: &Tensor, idx: &Tensor) -> Result<Tensor> {
    let rank = t.shape.len();
    let a = axis(n.int("axis", 0), rank)?;
    let dim = t.shape[a] as i64;
    let ids: Vec<usize> = ints(idx)?
        .iter()
        .map(|&i| {
            let j = if i < 0 { i + dim } else { i };
            if j < 0 || j >= dim {
                return err(format!("index {i} is out of range for size {dim}"));
            }
            Ok(j as usize)
        })
        .collect::<Result<_>>()?;
    let outer: usize = t.shape[..a].iter().product();
    let inner: usize = t.shape[a + 1..].iter().product();
    let mut shape = t.shape[..a].to_vec();
    shape.extend_from_slice(&idx.shape);
    shape.extend_from_slice(&t.shape[a + 1..]);
    macro_rules! pick {
        ($variant:ident, $v:expr) => {{
            let mut out = Vec::with_capacity(outer * ids.len() * inner);
            for o in 0..outer {
                for &i in &ids {
                    let start = (o * t.shape[a] + i) * inner;
                    out.extend_from_slice(&$v[start..start + inner]);
                }
            }
            Data::$variant(out)
        }};
    }
    let data = match &t.data {
        Data::F32(v) => pick!(F32, v),
        Data::I64(v) => pick!(I64, v),
        Data::I32(v) => pick!(I32, v),
        Data::U8(v) => pick!(U8, v),
        Data::I8(v) => pick!(I8, v),
        Data::Bool(v) => pick!(Bool, v),
    };
    Tensor::new(shape, data)
}

fn slice(inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let t = need(inputs, 0)?;
    let starts = ints(need(inputs, 1)?)?;
    let ends = ints(need(inputs, 2)?)?;
    let rank = t.shape.len();
    let axes: Vec<usize> = match opt(inputs, 3) {
        Some(a) => ints(a)?
            .iter()
            .map(|&a| axis(a, rank))
            .collect::<Result<_>>()?,
        None => (0..starts.len()).collect(),
    };
    let steps = match opt(inputs, 4) {
        Some(s) => ints(s)?,
        None => vec![1; starts.len()],
    };
    let mut begin = vec![0usize; rank];
    let mut shape = t.shape.clone();
    for (k, &a) in axes.iter().enumerate() {
        if steps.get(k).copied().unwrap_or(1) != 1 {
            return err("Slice with a step other than 1 is not supported");
        }
        let d = t.shape[a] as i64;
        let fix = |v: i64| (if v < 0 { v + d } else { v }).clamp(0, d);
        let s = fix(*starts.get(k).ok_or_else(|| Error("bad starts".into()))?);
        let e = fix(*ends.get(k).ok_or_else(|| Error("bad ends".into()))?);
        begin[a] = s as usize;
        shape[a] = (e - s).max(0) as usize;
    }
    let st = strides(&t.shape);
    let base: usize = begin.iter().zip(&st).map(|(b, s)| b * s).sum();
    macro_rules! cut {
        ($variant:ident, $v:expr) => {{
            let mut out = Vec::with_capacity(shape.iter().product());
            walk(&shape, std::slice::from_ref(&st), |offs, n, s| {
                let o = base + offs[0];
                out.extend((0..n).map(|i| $v[o + i * s[0]]));
            });
            Data::$variant(out)
        }};
    }
    let data = match &t.data {
        Data::F32(v) => cut!(F32, v),
        Data::I64(v) => cut!(I64, v),
        Data::I32(v) => cut!(I32, v),
        Data::U8(v) => cut!(U8, v),
        Data::I8(v) => cut!(I8, v),
        Data::Bool(v) => cut!(Bool, v),
    };
    Tensor::new(shape, data)
}

fn cast(t: &Tensor, to: i64) -> Result<Tensor> {
    let as_f64: Vec<f64> = match &t.data {
        Data::F32(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Data::I64(v) => v.iter().map(|&x| x as f64).collect(),
        Data::I32(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Data::U8(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Data::I8(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Data::Bool(v) => v.iter().map(|&x| f64::from(u8::from(x))).collect(),
    };
    let data = match to {
        1 => match &t.data {
            Data::I32(v) => Data::F32(v.iter().map(|&x| x as f32).collect()),
            _ => Data::F32(as_f64.iter().map(|&x| x as f32).collect()),
        },
        7 => match &t.data {
            Data::I64(v) => Data::I64(v.clone()),
            _ => Data::I64(as_f64.iter().map(|&x| x as i64).collect()),
        },
        6 => Data::I32(as_f64.iter().map(|&x| x as i32).collect()),
        2 => Data::U8(as_f64.iter().map(|&x| x as u8).collect()),
        3 => Data::I8(as_f64.iter().map(|&x| x as i8).collect()),
        9 => Data::Bool(as_f64.iter().map(|&x| x != 0.0).collect()),
        other => return err(format!("Cast to element type {other} is not supported")),
    };
    Tensor::new(t.shape.clone(), data)
}

/// ONNX `DynamicQuantizeLinear`: uint8 with a scale and zero point that
/// cover the data's range and zero.
fn dynamic_quantize(t: &Tensor) -> Result<Vec<Tensor>> {
    let v = f32s(t)?;
    let (lo, hi) = v
        .iter()
        .fold((0.0f32, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    let scale = (hi - lo) / 255.0;
    let (zero, q) = if scale > 0.0 && scale.is_finite() {
        let zero = (-lo / scale).round_ties_even().clamp(0.0, 255.0);
        let q: Vec<u8> = v
            .iter()
            .map(|&x| ((x / scale).round_ties_even() + zero).clamp(0.0, 255.0) as u8)
            .collect();
        (zero as u8, q)
    } else {
        (0, vec![0; v.len()])
    };
    Ok(vec![
        Tensor::new(t.shape.clone(), Data::U8(q))?,
        Tensor::f32(vec![], vec![scale])?,
        Tensor::new(vec![], Data::U8(vec![zero]))?,
    ])
}

fn zero_points(t: &Tensor) -> Result<Vec<f32>> {
    Ok(match &t.data {
        Data::U8(v) => v.iter().map(|&x| f32::from(x)).collect(),
        Data::I8(v) => v.iter().map(|&x| f32::from(x)).collect(),
        d => return err(format!("zero point of type {}", d.kind())),
    })
}

fn u8_or_i8(t: &Tensor) -> Result<Vec<f32>> {
    Ok(match &t.data {
        Data::U8(v) => v.iter().map(|&x| f32::from(x)).collect(),
        Data::I8(v) => v.iter().map(|&x| f32::from(x)).collect(),
        d => return err(format!("expected 8-bit integers, got {}", d.kind())),
    })
}

/// Integer weights `[O, ...]` less their zero point (one, or one per row).
fn dequant_rows(w: &Tensor, zp: &[f32]) -> Result<Vec<f32>> {
    let mut v = u8_or_i8(w)?;
    let rows = w.shape.first().copied().unwrap_or(1).max(1);
    let per = v.len() / rows;
    for (r, row) in v.chunks_mut(per.max(1)).enumerate() {
        let z = if zp.len() > 1 {
            zp[r]
        } else {
            zp.first().copied().unwrap_or(0.0)
        };
        for x in row {
            *x -= z;
        }
    }
    Ok(v)
}

/// C = A · B for row-major A (m×k) and B (k×n) given by strides.
#[allow(clippy::too_many_arguments)]
fn gemm(
    m: usize,
    k: usize,
    n: usize,
    a: &[f32],
    (rsa, csa): (usize, usize),
    b: &[f32],
    (rsb, csb): (usize, usize),
    c: &mut [f32],
    (rsc, csc): (usize, usize),
) {
    if m == 0 || n == 0 {
        return;
    }
    let last = |r: usize, cs: usize, rows: usize, cols: usize| (rows - 1) * r + (cols - 1) * cs;
    assert!(k == 0 || (last(rsa, csa, m, k) < a.len() && last(rsb, csb, k, n) < b.len()));
    assert!(last(rsc, csc, m, n) < c.len());
    // SAFETY: the asserts above keep every index matrixmultiply reads or
    // writes (row * stride + col * stride) inside the three slices.
    unsafe {
        matrixmultiply::sgemm(
            m,
            k,
            n,
            1.0,
            a.as_ptr(),
            rsa as isize,
            csa as isize,
            b.as_ptr(),
            rsb as isize,
            csb as isize,
            0.0,
            c.as_mut_ptr(),
            rsc as isize,
            csc as isize,
        );
    }
}

/// Batched `[.., m, k] × [.., k, n]` (NumPy rules, 1-D sides promoted).
fn batched_matmul(
    ash: &[usize],
    a: &[f32],
    bsh: &[usize],
    b: &[f32],
) -> Result<(Vec<usize>, Vec<f32>)> {
    let (mut ash, mut bsh) = (ash.to_vec(), bsh.to_vec());
    let a1 = ash.len() == 1;
    let b1 = bsh.len() == 1;
    if a1 {
        ash.insert(0, 1);
    }
    if b1 {
        bsh.push(1);
    }
    let (m, k) = (ash[ash.len() - 2], ash[ash.len() - 1]);
    let (k2, n) = (bsh[bsh.len() - 2], bsh[bsh.len() - 1]);
    if k != k2 {
        return err(format!("cannot multiply {ash:?} by {bsh:?}"));
    }
    let batch = broadcast_shape(&ash[..ash.len() - 2], &bsh[..bsh.len() - 2])?;
    let sa = broadcast_strides(&ash[..ash.len() - 2], &batch);
    let sb = broadcast_strides(&bsh[..bsh.len() - 2], &batch);
    let count: usize = batch.iter().product();
    let mut out = vec![0.0f32; count * m * n];
    let bst = strides(&batch);
    for i in 0..count {
        let (mut oa, mut ob) = (0, 0);
        for (d, &s) in bst.iter().enumerate() {
            let x = (i / s) % batch[d];
            oa += x * sa[d];
            ob += x * sb[d];
        }
        let a_off = oa * m * k;
        let b_off = ob * k * n;
        gemm(
            m,
            k,
            n,
            &a[a_off..a_off + m * k],
            (k, 1),
            &b[b_off..b_off + k * n],
            (n, 1),
            &mut out[i * m * n..(i + 1) * m * n],
            (n, 1),
        );
    }
    let mut shape = batch;
    if !a1 {
        shape.push(m);
    }
    if !b1 {
        shape.push(n);
    }
    Ok((shape, out))
}

fn matmul(a: &Tensor, b: &Tensor) -> Result<Tensor> {
    let (shape, out) = batched_matmul(&a.shape, f32s(a)?, &b.shape, f32s(b)?)?;
    Tensor::f32(shape, out)
}

fn matmul_integer(inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let a = need(inputs, 0)?;
    let b = need(inputs, 1)?;
    let az = opt(inputs, 2)
        .map(zero_points)
        .transpose()?
        .unwrap_or_default();
    let bz = opt(inputs, 3)
        .map(zero_points)
        .transpose()?
        .unwrap_or_default();
    if az.len() > 1 {
        return err("a per-row zero point for A is not supported");
    }
    let az = az.first().copied().unwrap_or(0.0);
    let mut af = u8_or_i8(a)?;
    for x in &mut af {
        *x -= az;
    }
    // B less its zero point, per column when there is one per column.
    let mut bf = u8_or_i8(b)?;
    let n = *b.shape.last().unwrap_or(&1);
    if bz.len() > 1 {
        if bz.len() != n {
            return err("B's zero points do not match its columns");
        }
        for row in bf.chunks_mut(n.max(1)) {
            for (x, z) in row.iter_mut().zip(&bz) {
                *x -= z;
            }
        }
    } else if let Some(&z) = bz.first() {
        for x in &mut bf {
            *x -= z;
        }
    }
    let (shape, out) = batched_matmul(&a.shape, &af, &b.shape, &bf)?;
    Tensor::new(
        shape,
        Data::I32(out.iter().map(|&v| v.round() as i32).collect()),
    )
}

struct ConvGeom {
    c: usize,
    h: usize,
    w: usize,
    o: usize,
    kh: usize,
    kw: usize,
    sh: usize,
    sw: usize,
    dh: usize,
    dw: usize,
    pt: usize,
    pl: usize,
    oh: usize,
    ow: usize,
}

fn conv_geom(n: &Node, x: &[usize], w: &[usize]) -> Result<ConvGeom> {
    let ([_, c, h, wd], [o, ci, kh, kw]) = (x, w) else {
        return err("only 2-D convolutions are supported");
    };
    if n.int("group", 1) != 1 || ci != c {
        return err("grouped convolutions are not supported");
    }
    let s = n.ints("strides").map_or(vec![1, 1], <[i64]>::to_vec);
    let d = n.ints("dilations").map_or(vec![1, 1], <[i64]>::to_vec);
    let p = n.ints("pads").map_or(vec![0, 0, 0, 0], <[i64]>::to_vec);
    if n.string("auto_pad").is_some_and(|a| a != "NOTSET") {
        return err("auto_pad is not supported");
    }
    let u = |v: &[i64], i: usize| v.get(i).copied().unwrap_or(0).max(0) as usize;
    let (sh, sw, dh, dw) = (
        u(&s, 0).max(1),
        u(&s, 1).max(1),
        u(&d, 0).max(1),
        u(&d, 1).max(1),
    );
    let (pt, pl, pb, pr) = (u(&p, 0), u(&p, 1), u(&p, 2), u(&p, 3));
    let span_h = dh * (kh - 1) + 1;
    let span_w = dw * (kw - 1) + 1;
    if h + pt + pb < span_h || wd + pl + pr < span_w {
        return err("the kernel is larger than the input");
    }
    Ok(ConvGeom {
        c: *c,
        h: *h,
        w: *wd,
        o: *o,
        kh: *kh,
        kw: *kw,
        sh,
        sw,
        dh,
        dw,
        pt,
        pl,
        oh: (h + pt + pb - span_h) / sh + 1,
        ow: (wd + pl + pr - span_w) / sw + 1,
    })
}

/// A 2-D convolution (NCHW) of `x` by weights `[O, C, kh, kw]`, batch by
/// batch, as products of the weights with tiles of input columns.
fn conv(n: &Node, xsh: &[usize], x: &[f32], wsh: &[usize], w: &[f32]) -> Result<Tensor> {
    let g = conv_geom(n, xsh, wsh)?;
    let batch = xsh[0];
    let r = g.c * g.kh * g.kw;
    let plane = g.oh * g.ow;
    let mut out = vec![0.0f32; batch * g.o * plane];
    let pointwise = g.kh == 1 && g.kw == 1 && g.sh == 1 && g.sw == 1 && g.pt == 0 && g.pl == 0;
    const TILE: usize = 4096;
    let mut col = if pointwise {
        Vec::new()
    } else {
        vec![0.0f32; r * TILE.min(plane)]
    };
    for bi in 0..batch {
        let xb = &x[bi * g.c * g.h * g.w..(bi + 1) * g.c * g.h * g.w];
        let ob = &mut out[bi * g.o * plane..(bi + 1) * g.o * plane];
        if pointwise {
            gemm(g.o, g.c, plane, w, (r, 1), xb, (plane, 1), ob, (plane, 1));
            continue;
        }
        let mut p0 = 0;
        while p0 < plane {
            let cols = TILE.min(plane - p0);
            for ci in 0..g.c {
                for ky in 0..g.kh {
                    for kx in 0..g.kw {
                        let row = (ci * g.kh + ky) * g.kw + kx;
                        let dst = &mut col[row * cols..(row + 1) * cols];
                        for (j, d) in dst.iter_mut().enumerate() {
                            let p = p0 + j;
                            let (oy, ox) = (p / g.ow, p % g.ow);
                            let iy = (oy * g.sh + ky * g.dh) as isize - g.pt as isize;
                            let ix = (ox * g.sw + kx * g.dw) as isize - g.pl as isize;
                            *d = if iy >= 0 && ix >= 0 && (iy as usize) < g.h && (ix as usize) < g.w
                            {
                                xb[(ci * g.h + iy as usize) * g.w + ix as usize]
                            } else {
                                0.0
                            };
                        }
                    }
                }
            }
            gemm(
                g.o,
                r,
                cols,
                w,
                (r, 1),
                &col[..r * cols],
                (cols, 1),
                &mut ob[p0..],
                (plane, 1),
            );
            p0 += cols;
        }
    }
    Tensor::f32(vec![batch, g.o, g.oh, g.ow], out)
}

fn add_channel_bias(t: &mut Tensor, b: &[f32]) -> Result<()> {
    let Data::F32(v) = &mut t.data else {
        return err("bias on a non-float tensor");
    };
    let o = t.shape[1];
    if b.len() != o {
        return err("the bias does not match the channels");
    }
    let plane: usize = t.shape[2..].iter().product();
    for (i, chunk) in v.chunks_mut(plane.max(1)).enumerate() {
        let bias = b[i % o];
        for x in chunk {
            *x += bias;
        }
    }
    Ok(())
}

/// A transposed 2-D convolution (weights `[C, O, kh, kw]`, one group).
fn conv_transpose(n: &Node, inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let x = need(inputs, 0)?;
    let wt = need(inputs, 1)?;
    let ([batch, c, h, wd], [ci, o, kh, kw]) = (x.shape.as_slice(), wt.shape.as_slice()) else {
        return err("only 2-D transposed convolutions are supported");
    };
    if n.int("group", 1) != 1 || ci != c {
        return err("grouped transposed convolutions are not supported");
    }
    if n.ints("dilations")
        .is_some_and(|d| d.iter().any(|&v| v != 1))
        || n.ints("output_shape").is_some()
    {
        return err("this transposed convolution is not supported");
    }
    let s = n.ints("strides").map_or(vec![1, 1], <[i64]>::to_vec);
    let p = n.ints("pads").map_or(vec![0, 0, 0, 0], <[i64]>::to_vec);
    let op = n.ints("output_padding").map_or(vec![0, 0], <[i64]>::to_vec);
    let u = |v: &[i64], i: usize| v.get(i).copied().unwrap_or(0).max(0) as usize;
    let (sh, sw) = (u(&s, 0).max(1), u(&s, 1).max(1));
    let (pt, pl, pb, pr) = (u(&p, 0), u(&p, 1), u(&p, 2), u(&p, 3));
    let oh = ((h - 1) * sh + kh + u(&op, 0))
        .checked_sub(pt + pb)
        .ok_or_else(|| Error("bad pads".into()))?;
    let ow = ((wd - 1) * sw + kw + u(&op, 1))
        .checked_sub(pl + pr)
        .ok_or_else(|| Error("bad pads".into()))?;
    let (xv, wv) = (f32s(x)?, f32s(wt)?);
    let rows = o * kh * kw;
    let hw = h * wd;
    let mut col = vec![0.0f32; rows * hw];
    let mut out = vec![0.0f32; batch * o * oh * ow];
    for bi in 0..*batch {
        // col[(o, ky, kx), pixel] = Σ_c W[c, (o, ky, kx)] · X[c, pixel]
        gemm(
            rows,
            *c,
            hw,
            wv,
            (1, rows),
            &xv[bi * c * hw..(bi + 1) * c * hw],
            (hw, 1),
            &mut col,
            (hw, 1),
        );
        let ob = &mut out[bi * o * oh * ow..(bi + 1) * o * oh * ow];
        for oc in 0..*o {
            for ky in 0..*kh {
                for kx in 0..*kw {
                    let src = &col[((oc * kh + ky) * kw + kx) * hw..][..hw];
                    for iy in 0..*h {
                        let y = (iy * sh + ky) as isize - pt as isize;
                        if y < 0 || y as usize >= oh {
                            continue;
                        }
                        for ix in 0..*wd {
                            let xo = (ix * sw + kx) as isize - pl as isize;
                            if xo < 0 || xo as usize >= ow {
                                continue;
                            }
                            ob[(oc * oh + y as usize) * ow + xo as usize] += src[iy * wd + ix];
                        }
                    }
                }
            }
        }
    }
    let mut t = Tensor::f32(vec![*batch, *o, oh, ow], out)?;
    if let Some(b) = opt(inputs, 2) {
        add_channel_bias(&mut t, f32s(b)?)?;
    }
    Ok(t)
}

/// Source taps and weights along one axis of a resize.
fn resize_taps(
    n_in: usize,
    n_out: usize,
    scale: f32,
    mode: &str,
    coord: &str,
    a: f32,
) -> Result<Vec<([usize; 4], [f32; 4])>> {
    let last = n_in.saturating_sub(1);
    (0..n_out)
        .map(|x| {
            let xf = x as f32;
            let orig = match coord {
                "half_pixel" => (xf + 0.5) / scale - 0.5,
                "pytorch_half_pixel" => {
                    if n_out > 1 {
                        (xf + 0.5) / scale - 0.5
                    } else {
                        0.0
                    }
                }
                "align_corners" => {
                    if n_out > 1 {
                        xf * last as f32 / (n_out - 1) as f32
                    } else {
                        0.0
                    }
                }
                "asymmetric" => xf / scale,
                other => return err(format!("coordinate mode {other} is not supported")),
            };
            Ok(match mode {
                "linear" => {
                    let o = orig.clamp(0.0, last as f32);
                    let i0 = o.floor() as usize;
                    let i1 = (i0 + 1).min(last);
                    let t = o - i0 as f32;
                    ([i0, i1, i1, i1], [1.0 - t, t, 0.0, 0.0])
                }
                "cubic" => {
                    let f = orig.floor();
                    let t = orig - f;
                    let base = f as isize;
                    let w = [
                        ((a * (t + 1.0) - 5.0 * a) * (t + 1.0) + 8.0 * a) * (t + 1.0) - 4.0 * a,
                        ((a + 2.0) * t - (a + 3.0)) * t * t + 1.0,
                        ((a + 2.0) * (1.0 - t) - (a + 3.0)) * (1.0 - t) * (1.0 - t) + 1.0,
                        ((a * (2.0 - t) - 5.0 * a) * (2.0 - t) + 8.0 * a) * (2.0 - t) - 4.0 * a,
                    ];
                    let idx = |d: isize| (base + d).clamp(0, last as isize) as usize;
                    ([idx(-1), idx(0), idx(1), idx(2)], w)
                }
                "nearest" => {
                    let i = (orig.floor().max(0.0) as usize).min(last);
                    ([i, i, i, i], [1.0, 0.0, 0.0, 0.0])
                }
                other => return err(format!("resize mode {other} is not supported")),
            })
        })
        .collect()
}

fn resize(n: &Node, inputs: &[Option<Arc<Tensor>>]) -> Result<Tensor> {
    let x = need(inputs, 0)?;
    let [b, c, h, w] = x.shape.as_slice() else {
        return err("only 4-D resizes are supported");
    };
    let (b, c, h, w) = (*b, *c, *h, *w);
    let (oh, ow, sy, sx) = if let Some(sizes) = opt(inputs, 3).filter(|t| t.len() > 0) {
        let s = ints(sizes)?;
        let [sb, sc, sh, sw] = s.as_slice() else {
            return err("sizes must have four values");
        };
        if *sb as usize != b || *sc as usize != c {
            return err("only the spatial axes may be resized");
        }
        let (sh, sw) = (*sh as usize, *sw as usize);
        (sh, sw, sh as f32 / h as f32, sw as f32 / w as f32)
    } else if let Some(scales) = opt(inputs, 2).filter(|t| t.len() > 0) {
        let s = f32s(scales)?;
        let [s0, s1, s2, s3] = s else {
            return err("scales must have four values");
        };
        if *s0 != 1.0 || *s1 != 1.0 {
            return err("only the spatial axes may be resized");
        }
        (
            (h as f32 * s2).floor() as usize,
            (w as f32 * s3).floor() as usize,
            *s2,
            *s3,
        )
    } else {
        return err("neither scales nor sizes");
    };
    let mode = n.string("mode").unwrap_or("nearest");
    let coord = n
        .string("coordinate_transformation_mode")
        .unwrap_or("half_pixel");
    let a = n.float("cubic_coeff_a", -0.75);
    if n.int("exclude_outside", 0) != 0 || n.int("antialias", 0) != 0 {
        return err("exclude_outside and antialias are not supported");
    }
    let ty = resize_taps(h, oh, sy, mode, coord, a)?;
    let tx = resize_taps(w, ow, sx, mode, coord, a)?;
    let v = f32s(x)?;
    let mut out = vec![0.0f32; b * c * oh * ow];
    let mut row = vec![0.0f32; ow];
    for plane in 0..b * c {
        let src = &v[plane * h * w..(plane + 1) * h * w];
        let dst = &mut out[plane * oh * ow..(plane + 1) * oh * ow];
        // Horizontal pass per source row, then the vertical blend.
        let mut horiz = vec![0.0f32; h * ow];
        for y in 0..h {
            let srow = &src[y * w..(y + 1) * w];
            for (xo, (idx, wt)) in tx.iter().enumerate() {
                row[xo] = srow[idx[0]] * wt[0]
                    + srow[idx[1]] * wt[1]
                    + srow[idx[2]] * wt[2]
                    + srow[idx[3]] * wt[3];
            }
            horiz[y * ow..(y + 1) * ow].copy_from_slice(&row);
        }
        for (yo, (idx, wt)) in ty.iter().enumerate() {
            let d = &mut dst[yo * ow..(yo + 1) * ow];
            for (k, &i) in idx.iter().enumerate() {
                if wt[k] == 0.0 {
                    continue;
                }
                let s = &horiz[i * ow..(i + 1) * ow];
                for (o, &v) in d.iter_mut().zip(s) {
                    *o += v * wt[k];
                }
            }
        }
    }
    Tensor::f32(vec![b, c, oh, ow], out)
}
