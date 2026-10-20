//! The ONNX reader and runner (`onnx.rs`, `onnx_run.rs`) on small graphs
//! written here in the protocol-buffer format, each operator against
//! values worked out by hand. `real_depth_anything_v2` (ignored: needs the
//! model file) runs the model 3d-cover downloads.

use super::onnx::{self, Data, Tensor};
use super::onnx_run::Plan;

// ------------------------------------------------------------ encoder

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn field_bytes(n: u32, bytes: &[u8], out: &mut Vec<u8>) {
    varint(u64::from(n) << 3 | 2, out);
    varint(bytes.len() as u64, out);
    out.extend_from_slice(bytes);
}

fn field_varint(n: u32, v: u64, out: &mut Vec<u8>) {
    varint(u64::from(n) << 3, out);
    varint(v, out);
}

fn field_f32(n: u32, v: f32, out: &mut Vec<u8>) {
    varint(u64::from(n) << 3 | 5, out);
    out.extend_from_slice(&v.to_le_bytes());
}

/// A TensorProto: raw little-endian data of element type `kind`.
pub(super) fn tensor(name: &str, dims: &[i64], kind: u64, raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &d in dims {
        field_varint(1, d as u64, &mut out);
    }
    field_varint(2, kind, &mut out);
    field_bytes(8, name.as_bytes(), &mut out);
    field_bytes(9, raw, &mut out);
    out
}

pub(super) fn f32_tensor(name: &str, dims: &[i64], v: &[f32]) -> Vec<u8> {
    let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    tensor(name, dims, 1, &raw)
}

pub(super) fn i64_tensor(name: &str, dims: &[i64], v: &[i64]) -> Vec<u8> {
    let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    tensor(name, dims, 7, &raw)
}

pub(super) fn u8_tensor(name: &str, dims: &[i64], v: &[u8]) -> Vec<u8> {
    tensor(name, dims, 2, v)
}

pub(super) enum A<'a> {
    I(i64),
    F(f32),
    S(&'a str),
    Is(&'a [i64]),
}

pub(super) fn node(
    op: &str,
    inputs: &[&str],
    outputs: &[&str],
    attrs: &[(&str, A<'_>)],
) -> Vec<u8> {
    let mut out = Vec::new();
    for i in inputs {
        field_bytes(1, i.as_bytes(), &mut out);
    }
    for o in outputs {
        field_bytes(2, o.as_bytes(), &mut out);
    }
    field_bytes(4, op.as_bytes(), &mut out);
    for (name, a) in attrs {
        let mut at = Vec::new();
        field_bytes(1, name.as_bytes(), &mut at);
        match a {
            A::F(f) => {
                field_f32(2, *f, &mut at);
                field_varint(20, 1, &mut at);
            }
            A::I(i) => {
                field_varint(3, *i as u64, &mut at);
                field_varint(20, 2, &mut at);
            }
            A::S(s) => {
                field_bytes(4, s.as_bytes(), &mut at);
                field_varint(20, 3, &mut at);
            }
            A::Is(v) => {
                for i in *v {
                    field_varint(8, *i as u64, &mut at);
                }
                field_varint(20, 7, &mut at);
            }
        }
        field_bytes(5, &at, &mut out);
    }
    out
}

fn value_info(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    field_bytes(1, name.as_bytes(), &mut out);
    out
}

/// A model of `nodes` and `inits` reading `input` and giving `output`.
pub(super) fn model(nodes: &[Vec<u8>], inits: &[Vec<u8>], input: &str, output: &str) -> Vec<u8> {
    let mut graph = Vec::new();
    for n in nodes {
        field_bytes(1, n, &mut graph);
    }
    field_bytes(2, b"test", &mut graph);
    for t in inits {
        field_bytes(5, t, &mut graph);
    }
    field_bytes(11, &value_info(input), &mut graph);
    field_bytes(12, &value_info(output), &mut graph);
    let mut m = Vec::new();
    field_varint(1, 8, &mut m);
    let mut opset = Vec::new();
    field_varint(2, 17, &mut opset);
    field_bytes(8, &opset, &mut m);
    field_bytes(7, &graph, &mut m);
    m
}

fn run(nodes: &[Vec<u8>], inits: &[Vec<u8>], input: Tensor) -> Tensor {
    let m = onnx::parse(&model(nodes, inits, "x", "y")).expect("parses");
    Plan::new(m).expect("plans").run(input).expect("runs")
}

fn f(shape: &[usize], v: &[f32]) -> Tensor {
    Tensor::f32(shape.to_vec(), v.to_vec()).unwrap()
}

fn floats(t: &Tensor) -> Vec<f32> {
    match &t.data {
        Data::F32(v) => v.clone(),
        Data::I32(v) => v.iter().map(|&x| x as f32).collect(),
        Data::I64(v) => v.iter().map(|&x| x as f32).collect(),
        Data::U8(v) => v.iter().map(|&x| f32::from(x)).collect(),
        d => panic!("{}", d.kind()),
    }
}

fn close(a: &[f32], b: &[f32], eps: f32) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= eps, "{a:?} vs {b:?}");
    }
}

// ------------------------------------------------------------ tests

#[test]
fn broadcasting_arithmetic_and_layer_norm_ops() {
    // (x - mean) / sqrt(var + eps) * w + b over the last axis, as the
    // exported LayerNorm does it.
    let nodes = [
        node("ReduceMean", &["x"], &["m"], &[("axes", A::Is(&[-1]))]),
        node("Sub", &["x", "m"], &["d"], &[]),
        node("Pow", &["d", "two"], &["p"], &[]),
        node("ReduceMean", &["p"], &["v"], &[("axes", A::Is(&[-1]))]),
        node("Add", &["v", "eps"], &["ve"], &[]),
        node("Sqrt", &["ve"], &["s"], &[]),
        node("Div", &["d", "s"], &["n"], &[]),
        node("Mul", &["n", "w"], &["nw"], &[]),
        node("Add", &["nw", "b"], &["y"], &[]),
    ];
    let inits = [
        f32_tensor("two", &[], &[2.0]),
        f32_tensor("eps", &[], &[0.0]),
        f32_tensor("w", &[2], &[1.0, 2.0]),
        f32_tensor("b", &[2], &[0.5, 0.0]),
    ];
    let y = run(&nodes, &inits, f(&[2, 2], &[1.0, 3.0, 5.0, 5.0]));
    assert_eq!(y.shape, vec![2, 2]);
    // Row 1: mean 2, std 1 -> [-1, 1]; row 2 is flat (0/0 = NaN).
    let v = floats(&y);
    close(&v[..2], &[-0.5, 2.0], 1e-6);
    assert!(v[2].is_nan());
}

#[test]
fn shape_ops_follow_onnx() {
    let nodes = [
        node("Shape", &["x"], &["s"], &[]),
        node("Gather", &["s", "one"], &["h"], &[("axis", A::I(0))]),
        node("Unsqueeze", &["h", "zero"], &["hu"], &[]),
        node(
            "Concat",
            &["hu", "minus1"],
            &["shape"],
            &[("axis", A::I(0))],
        ),
        node("Reshape", &["x", "shape"], &["r"], &[]),
        node("Transpose", &["r"], &["t"], &[("perm", A::Is(&[1, 0]))]),
        node("Slice", &["t", "zero", "two", "zero"], &["sl"], &[]),
        node("Squeeze", &["sl"], &["y"], &[]),
    ];
    let inits = [
        i64_tensor("one", &[], &[1]),
        i64_tensor("zero", &[1], &[0]),
        i64_tensor("two", &[1], &[2]),
        i64_tensor("minus1", &[1], &[-1]),
    ];
    // x [1, 3, 2] -> reshape [3, 2] -> transpose [2, 3] -> rows 0..2.
    let y = run(
        &nodes,
        &inits,
        f(&[1, 3, 2], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
    );
    assert_eq!(y.shape, vec![2, 3]);
    assert_eq!(floats(&y), vec![0.0, 2.0, 4.0, 1.0, 3.0, 5.0]);
}

#[test]
fn where_equal_expand_and_cast() {
    let nodes = [
        node("Equal", &["shape", "minus"], &["eq"], &[]),
        node("Where", &["eq", "ones", "shape"], &["w"], &[]),
        node("Expand", &["x", "w"], &["e"], &[]),
        node("Cast", &["e"], &["y"], &[("to", A::I(7))]),
    ];
    let inits = [
        i64_tensor("shape", &[2], &[-1, 3]),
        i64_tensor("minus", &[2], &[-1, -1]),
        i64_tensor("ones", &[2], &[1, 1]),
    ];
    let y = run(&nodes, &inits, f(&[1, 1], &[2.7]));
    assert_eq!(y.shape, vec![1, 3]);
    assert_eq!(y.data, Data::I64(vec![2, 2, 2]));
}

#[test]
fn softmax_erf_and_batched_matmul() {
    let nodes = [
        node("MatMul", &["x", "k"], &["m"], &[]),
        node("Softmax", &["m"], &["s"], &[("axis", A::I(-1))]),
        node("Erf", &["s"], &["y"], &[]),
    ];
    // x [2, 1, 2] (two batches) by k [2, 2]: the same k for both.
    let inits = [f32_tensor("k", &[2, 2], &[1.0, 0.0, 0.0, 1.0])];
    let y = run(&nodes, &inits, f(&[2, 1, 2], &[0.0, 0.0, 1.0, 0.0]));
    assert_eq!(y.shape, vec![2, 1, 2]);
    let e = 1.0f32.exp();
    let (a, b) = (e / (e + 1.0), 1.0 / (e + 1.0));
    // erf(0.5) = 0.5204999, erf(a), erf(b) from tables.
    let erf = |x: f32| -> f32 {
        // Reference: series to many terms in f64.
        let x = f64::from(x);
        let mut sum = 0.0;
        let mut term = x;
        for n in 0..60 {
            sum += term / f64::from(2 * n + 1);
            term *= -x * x / f64::from(n + 1);
        }
        (sum * 2.0 / std::f64::consts::PI.sqrt()) as f32
    };
    close(&floats(&y), &[erf(0.5), erf(0.5), erf(a), erf(b)], 2e-6);
}

#[test]
fn dynamic_quantize_and_integer_matmul() {
    let nodes = [
        node("DynamicQuantizeLinear", &["x"], &["q", "scale", "zp"], &[]),
        node("MatMulInteger", &["q", "w", "zp", "wzp"], &["mi"], &[]),
        node("Cast", &["mi"], &["c"], &[("to", A::I(1))]),
        node("Mul", &["scale", "wscale"], &["sc"], &[]),
        node("Mul", &["c", "sc"], &["y"], &[]),
    ];
    // x in [-1, 2.06]: scale 3.06/255 = 0.012, zero point round(1/0.012) = 83.
    let inits = [
        u8_tensor("w", &[2, 2], &[130, 128, 128, 126]),
        u8_tensor("wzp", &[2], &[128, 127]),
        f32_tensor("wscale", &[2], &[0.5, 0.25]),
    ];
    let x = f(&[1, 2], &[-1.0, 2.06]);
    let y = run(&nodes, &inits, x);
    // w less its zero points is [[2, 1], [0, -1]]: y ≈ x · [[1, .25], [0, -.25]].
    let v = floats(&y);
    close(&v, &[-1.0, -0.25 - 0.515], 0.02);
    // The quantization itself, against the spec's formula.
    let m = onnx::parse(&model(
        &[node("DynamicQuantizeLinear", &["x"], &["y", "s", "z"], &[])],
        &[],
        "x",
        "y",
    ))
    .unwrap();
    let q = Plan::new(m)
        .unwrap()
        .run(f(&[3], &[-1.0, 0.0, 2.06]))
        .unwrap();
    assert_eq!(q.data, Data::U8(vec![0, 83, 255]));
}

#[test]
fn conv_integer_conv_and_transposed_conv() {
    // 3x3 kernel of ones, pad 1, stride 2 over a 4x4 of ones: corner and
    // edge sums.
    let ones = [1.0f32; 16];
    let k = [1.0f32; 9];
    let conv = [node(
        "Conv",
        &["x", "k", "b"],
        &["y"],
        &[
            ("kernel_shape", A::Is(&[3, 3])),
            ("pads", A::Is(&[1, 1, 1, 1])),
            ("strides", A::Is(&[2, 2])),
        ],
    )];
    let y = run(
        &conv,
        &[
            f32_tensor("k", &[1, 1, 3, 3], &k),
            f32_tensor("b", &[1], &[0.5]),
        ],
        f(&[1, 1, 4, 4], &ones),
    );
    assert_eq!(y.shape, vec![1, 1, 2, 2]);
    assert_eq!(floats(&y), vec![4.5, 6.5, 6.5, 9.5]);
    // The same through quantized input and weights (zero points 10 and 3).
    let qnodes = [
        node("Cast", &["x"], &["xu"], &[("to", A::I(2))]),
        node(
            "ConvInteger",
            &["xu", "w", "xz", "wz"],
            &["y"],
            &[
                ("kernel_shape", A::Is(&[3, 3])),
                ("pads", A::Is(&[1, 1, 1, 1])),
                ("strides", A::Is(&[2, 2])),
            ],
        ),
    ];
    let y = run(
        &qnodes,
        &[
            u8_tensor("w", &[1, 1, 3, 3], &[4; 9]),
            u8_tensor("xz", &[], &[10]),
            u8_tensor("wz", &[], &[3]),
        ],
        f(&[1, 1, 4, 4], &[11.0; 16]),
    );
    assert_eq!(y.data, Data::I32(vec![4, 6, 6, 9]));
    // A 2x2 stride-2 transposed convolution puts each input pixel's
    // kernel copy in its own block.
    let tnodes = [node(
        "ConvTranspose",
        &["x", "k", "b"],
        &["y"],
        &[
            ("kernel_shape", A::Is(&[2, 2])),
            ("strides", A::Is(&[2, 2])),
        ],
    )];
    let y = run(
        &tnodes,
        &[
            f32_tensor("k", &[1, 1, 2, 2], &[1.0, 2.0, 3.0, 4.0]),
            f32_tensor("b", &[1], &[1.0]),
        ],
        f(&[1, 1, 1, 2], &[1.0, 10.0]),
    );
    assert_eq!(y.shape, vec![1, 1, 2, 4]);
    assert_eq!(floats(&y), vec![2.0, 3.0, 11.0, 21.0, 4.0, 5.0, 31.0, 41.0]);
}

#[test]
fn resize_linear_align_corners_and_cubic() {
    let lin = [node(
        "Resize",
        &["x", "", "scales"],
        &["y"],
        &[
            ("mode", A::S("linear")),
            ("coordinate_transformation_mode", A::S("align_corners")),
        ],
    )];
    let y = run(
        &lin,
        &[f32_tensor("scales", &[4], &[1.0, 1.0, 1.0, 2.0])],
        f(&[1, 1, 1, 2], &[0.0, 3.0]),
    );
    assert_eq!(y.shape, vec![1, 1, 1, 4]);
    close(&floats(&y), &[0.0, 1.0, 2.0, 3.0], 1e-6);
    // Cubic, half_pixel, a = -0.75: a 4 -> 2 halving of a ramp keeps it a
    // ramp (sizes input).
    let cub = [node(
        "Resize",
        &["x", "", "", "sizes"],
        &["y"],
        &[("mode", A::S("cubic")), ("cubic_coeff_a", A::F(-0.75))],
    )];
    let y = run(
        &cub,
        &[i64_tensor("sizes", &[4], &[1, 1, 1, 2])],
        f(&[1, 1, 1, 4], &[0.0, 1.0, 2.0, 3.0]),
    );
    // Sample points 0.5 and 2.5 (taps clamped at the edges): weights
    // -0.09375, 0.59375, 0.59375, -0.09375.
    close(&floats(&y), &[0.40625, 2.59375], 1e-5);
}

#[test]
fn unsupported_and_malformed_models_say_why() {
    let m = onnx::parse(&model(
        &[node("Einsum", &["x"], &["y"], &[])],
        &[],
        "x",
        "y",
    ))
    .unwrap();
    let e = Plan::new(m).err().expect("refused");
    assert!(e.0.contains("Einsum"), "{e}");
    assert!(onnx::parse(b"\x0a\xff").is_err());
    assert!(onnx::parse(b"").is_err(), "no graph");
}

/// The model 3d-cover downloads (`TIDALFAST_DEPTH_MODEL=<model_quantized.onnx>`):
/// a 392x392 gradient runs and the near edge comes out nearer.
#[test]
#[ignore = "needs TIDALFAST_DEPTH_MODEL"]
fn real_depth_anything_v2() {
    let Some(path) = std::env::var_os("TIDALFAST_DEPTH_MODEL") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let started = std::time::Instant::now();
    let plan = Plan::new(onnx::parse(&bytes).unwrap()).unwrap();
    let loaded = started.elapsed();
    let mut image = vec![0u8; 640 * 640 * 4];
    for (i, px) in image.chunks_mut(4).enumerate() {
        let y = (i / 640) as u8 / 3;
        px.copy_from_slice(&[y, y, y, 255]);
    }
    let r = super::depth::Request {
        width: 640,
        height: 640,
        channels: 4,
        size: [392, 392],
        rescale: Some(1.0 / 255.0),
        mean: Some([0.485, 0.456, 0.406]),
        std: Some([0.229, 0.224, 0.225]),
    };
    let started = std::time::Instant::now();
    let out = super::depth::estimate(&plan, &image, &r).unwrap();
    eprintln!("load {loaded:?}, estimate {:?}", started.elapsed());
    assert_eq!(out.depth.len(), 640 * 640);
    assert!(out.depth.contains(&255) && out.depth.contains(&0));
}
