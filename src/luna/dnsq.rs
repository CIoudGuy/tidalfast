//! The DNS client behind `dns.resolveMx`, `resolveTxt`, `resolveSrv`,
//! `reverse` and the other record types of Node's `dns` module. Plugins have
//! no UDP of their own (it is refused); the host asks instead, after the same
//! question a connection to the name would ask, and hands back plain JSON.
//!
//! Servers: the ones a plugin set with `setServers` (each asked as a
//! connection to that server), else the system's.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use serde_json::{Value, json};

/// How long one server gets for one try.
const TRY_FOR: Duration = Duration::from_millis(2500);

/// c-ares' error codes, as Node reports them.
#[derive(Debug, PartialEq, Eq)]
pub enum DnsError {
    NotFound,
    NoData,
    FormErr,
    ServFail,
    NotImp,
    Refused,
    BadResp,
    BadName,
    Timeout,
    ConnRefused,
}

impl DnsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "ENOTFOUND",
            Self::NoData => "ENODATA",
            Self::FormErr => "EFORMERR",
            Self::ServFail => "ESERVFAIL",
            Self::NotImp => "ENOTIMP",
            Self::Refused => "EREFUSED",
            Self::BadResp => "EBADRESP",
            Self::BadName => "EBADNAME",
            Self::Timeout => "ETIMEOUT",
            Self::ConnRefused => "ECONNREFUSED",
        }
    }
}

/// The record type number for Node's `rrtype` name.
pub fn qtype_of(name: &str) -> Option<u16> {
    Some(match name {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "SOA" => 6,
        "PTR" => 12,
        "MX" => 15,
        "TXT" => 16,
        "AAAA" => 28,
        "SRV" => 33,
        "NAPTR" => 35,
        "TLSA" => 52,
        "ANY" => 255,
        "CAA" => 257,
        _ => return None,
    })
}

fn type_name(t: u16) -> Option<&'static str> {
    Some(match t {
        1 => "A",
        2 => "NS",
        5 => "CNAME",
        6 => "SOA",
        12 => "PTR",
        15 => "MX",
        16 => "TXT",
        28 => "AAAA",
        33 => "SRV",
        35 => "NAPTR",
        52 => "TLSA",
        257 => "CAA",
        _ => return None,
    })
}

/// The `in-addr.arpa` / `ip6.arpa` name of an address, for `reverse`.
pub fn reverse_name(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(a) => {
            let mut s = String::new();
            for b in a.octets().iter().rev() {
                s.push_str(&format!("{:x}.{:x}.", b & 0xf, b >> 4));
            }
            s.push_str("ip6.arpa");
            s
        }
    }
}

/// A question for `name` of `qtype`, with recursion desired.
pub fn build_query(id: u16, name: &str, qtype: u16) -> Result<Vec<u8>, DnsError> {
    let name = name.trim_end_matches('.');
    if name.len() > 253 {
        return Err(DnsError::BadName);
    }
    let mut out = Vec::with_capacity(name.len() + 18);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x00]); // RD
    out.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    if !name.is_empty() {
        for label in name.split('.') {
            if label.is_empty() || label.len() > 63 {
                return Err(DnsError::BadName);
            }
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
    }
    out.push(0);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&[0, 1]); // IN
    Ok(out)
}

/// A name at `pos`, following compression pointers; returns the position
/// after it in the original run.
fn read_name(buf: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *buf.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let low = *buf.get(pos + 1)? as usize;
            end.get_or_insert(pos + 2);
            pos = ((len & 0x3f) << 8) | low;
            jumps += 1;
            if jumps > 64 {
                return None;
            }
            continue;
        }
        if len & 0xc0 != 0 {
            return None;
        }
        let label = buf.get(pos + 1..pos + 1 + len)?;
        if !out.is_empty() {
            out.push('.');
        }
        for &b in label {
            // c-ares writes a dot or backslash inside a label escaped.
            if b == b'.' || b == b'\\' {
                out.push('\\');
            }
            out.push(b as char);
        }
        pos += 1 + len;
    }
    Some((out, end.unwrap_or(pos)))
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]))
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(p)?,
        *b.get(p + 1)?,
        *b.get(p + 2)?,
        *b.get(p + 3)?,
    ]))
}

/// The record's data in the shape Node returns it (`{ exchange, priority }`
/// for MX, ...); `None` for a type this client does not read.
fn rdata(buf: &[u8], t: u16, start: usize, len: usize) -> Option<Value> {
    let end = start.checked_add(len)?;
    let data = buf.get(start..end)?;
    Some(match t {
        1 => {
            let o: [u8; 4] = data.try_into().ok()?;
            json!({ "address": Ipv4Addr::from(o).to_string() })
        }
        28 => {
            let o: [u8; 16] = data.try_into().ok()?;
            json!({ "address": Ipv6Addr::from(o).to_string() })
        }
        2 | 5 | 12 => json!({ "value": read_name(buf, start)?.0 }),
        15 => json!({ "priority": u16_at(buf, start)?, "exchange": read_name(buf, start + 2)?.0 }),
        16 => {
            let mut entries = Vec::new();
            let mut p = 0;
            while p < data.len() {
                let n = data[p] as usize;
                let chunk = data.get(p + 1..p + 1 + n)?;
                entries.push(String::from_utf8_lossy(chunk).into_owned());
                p += 1 + n;
            }
            json!({ "entries": entries })
        }
        33 => json!({
            "priority": u16_at(buf, start)?,
            "weight": u16_at(buf, start + 2)?,
            "port": u16_at(buf, start + 4)?,
            "name": read_name(buf, start + 6)?.0,
        }),
        6 => {
            let (ns, p) = read_name(buf, start)?;
            let (host, p) = read_name(buf, p)?;
            json!({
                "nsname": ns, "hostmaster": host,
                "serial": u32_at(buf, p)?, "refresh": u32_at(buf, p + 4)?,
                "retry": u32_at(buf, p + 8)?, "expire": u32_at(buf, p + 12)?,
                "minttl": u32_at(buf, p + 16)?,
            })
        }
        257 => {
            let critical = *data.first()? & 0x80 != 0;
            let tag_len = *data.get(1)? as usize;
            let tag = String::from_utf8_lossy(data.get(2..2 + tag_len)?).into_owned();
            let value = String::from_utf8_lossy(data.get(2 + tag_len..)?).into_owned();
            let mut o = serde_json::Map::new();
            o.insert("critical".into(), json!(u8::from(critical)));
            o.insert(tag, Value::String(value));
            Value::Object(o)
        }
        35 => {
            let order = u16_at(buf, start)?;
            let preference = u16_at(buf, start + 2)?;
            let mut p = start + 4;
            let mut strs = Vec::new();
            for _ in 0..3 {
                let n = *buf.get(p)? as usize;
                strs.push(String::from_utf8_lossy(buf.get(p + 1..p + 1 + n)?).into_owned());
                p += 1 + n;
            }
            let (replacement, _) = read_name(buf, p)?;
            json!({
                "flags": strs[0], "service": strs[1], "regexp": strs[2],
                "replacement": replacement, "order": order, "preference": preference,
            })
        }
        52 => json!({
            "certUsage": *data.first()?, "selector": *data.get(1)?, "match": *data.get(2)?,
            "data": data.get(3..)?.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        }),
        _ => return None,
    })
}

/// What a response holds for the question: records as
/// `{ type, ttl, data }`. A response cut short (`TC`) is `Ok(None)`: ask
/// again over TCP.
pub fn parse_response(buf: &[u8], id: u16, qtype: u16) -> Result<Option<Vec<Value>>, DnsError> {
    if buf.len() < 12 || u16_at(buf, 0) != Some(id) {
        return Err(DnsError::BadResp);
    }
    let flags = u16_at(buf, 2).ok_or(DnsError::BadResp)?;
    if flags & 0x8000 == 0 {
        return Err(DnsError::BadResp);
    }
    match flags & 0xf {
        0 => {}
        1 => return Err(DnsError::FormErr),
        2 => return Err(DnsError::ServFail),
        3 => return Err(DnsError::NotFound),
        4 => return Err(DnsError::NotImp),
        5 => return Err(DnsError::Refused),
        _ => return Err(DnsError::ServFail),
    }
    if flags & 0x0200 != 0 {
        return Ok(None);
    }
    let questions = u16_at(buf, 4).ok_or(DnsError::BadResp)? as usize;
    let answers = u16_at(buf, 6).ok_or(DnsError::BadResp)? as usize;
    let mut pos = 12;
    for _ in 0..questions {
        pos = read_name(buf, pos).ok_or(DnsError::BadResp)?.1 + 4;
    }
    let mut out = Vec::new();
    for _ in 0..answers {
        let (_, p) = read_name(buf, pos).ok_or(DnsError::BadResp)?;
        let t = u16_at(buf, p).ok_or(DnsError::BadResp)?;
        let ttl = u32_at(buf, p + 4).ok_or(DnsError::BadResp)?;
        let len = u16_at(buf, p + 8).ok_or(DnsError::BadResp)? as usize;
        let start = p + 10;
        pos = start + len;
        if pos > buf.len() {
            return Err(DnsError::BadResp);
        }
        let wanted = qtype == 255 || t == qtype;
        if !wanted {
            continue;
        }
        let (Some(name), Some(data)) = (type_name(t), rdata(buf, t, start, len)) else {
            continue;
        };
        out.push(json!({ "type": name, "ttl": ttl, "data": data }));
    }
    if out.is_empty() {
        return Err(DnsError::NoData);
    }
    Ok(Some(out))
}

fn fresh_id() -> u16 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.finish() as u16
}

fn ask_udp(server: SocketAddr, query: &[u8]) -> std::io::Result<Vec<u8>> {
    let bind: SocketAddr = if server.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(bind)?;
    socket.set_read_timeout(Some(TRY_FOR))?;
    socket.set_write_timeout(Some(TRY_FOR))?;
    socket.connect(server)?;
    socket.send(query)?;
    let mut buf = vec![0u8; 4096];
    let n = socket.recv(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

fn ask_tcp(server: SocketAddr, query: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect_timeout(&server, TRY_FOR)?;
    s.set_read_timeout(Some(TRY_FOR))?;
    s.set_write_timeout(Some(TRY_FOR))?;
    let mut framed = (query.len() as u16).to_be_bytes().to_vec();
    framed.extend_from_slice(query);
    s.write_all(&framed)?;
    let mut len = [0u8; 2];
    s.read_exact(&mut len)?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// Ask the servers in turn (two tries each over UDP, TCP when the answer is
/// cut short) until one gives a definite answer.
pub fn resolve(servers: &[SocketAddr], name: &str, qtype: u16) -> Result<Vec<Value>, DnsError> {
    if servers.is_empty() {
        return Err(DnsError::ConnRefused);
    }
    let mut last = DnsError::Timeout;
    for &server in servers {
        let id = fresh_id();
        let query = build_query(id, name, qtype)?;
        for _ in 0..2 {
            let reply = match ask_udp(server, &query) {
                Ok(r) => r,
                Err(e) => {
                    last = match e.kind() {
                        std::io::ErrorKind::ConnectionRefused => DnsError::ConnRefused,
                        _ => DnsError::Timeout,
                    };
                    continue;
                }
            };
            let parsed = match parse_response(&reply, id, qtype) {
                Ok(Some(records)) => return Ok(records),
                Ok(None) => ask_tcp(server, &query)
                    .map_err(|_| DnsError::Timeout)
                    .and_then(|r| parse_response(&r, id, qtype))
                    .and_then(|p| p.ok_or(DnsError::BadResp)),
                Err(e) => Err(e),
            };
            match parsed {
                Ok(records) => return Ok(records),
                // An answer: this is the result, no other server is asked.
                Err(
                    e @ (DnsError::NotFound
                    | DnsError::NoData
                    | DnsError::NotImp
                    | DnsError::Refused
                    | DnsError::FormErr),
                ) => return Err(e),
                // A broken or failing server: try the next.
                Err(e) => {
                    last = e;
                    break;
                }
            }
        }
    }
    Err(last)
}

/// `1.2.3.4`, `1.2.3.4:5353`, `::1`, `[::1]:53`.
pub fn parse_server(s: &str) -> Option<SocketAddr> {
    let s = s.trim();
    if let Ok(sa) = s.parse::<SocketAddr>() {
        return Some(sa);
    }
    s.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .ok()
        .map(|ip| SocketAddr::new(ip, 53))
}

/// The resolvers the system is configured with.
#[cfg(not(windows))]
pub fn system_servers() -> Vec<SocketAddr> {
    std::fs::read_to_string("/etc/resolv.conf")
        .map(|t| servers_of_resolv_conf(&t))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn servers_of_resolv_conf(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|l| {
            let l = l.split(['#', ';']).next()?.trim();
            l.strip_prefix("nameserver")?
                .trim()
                .split('%')
                .next()
                .and_then(parse_server)
        })
        .collect()
}

#[cfg(windows)]
pub fn system_servers() -> Vec<SocketAddr> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{FIXED_INFO_W2KSP1, GetNetworkParams};
    let mut len = 0u32;
    // SAFETY: the first call only reports the size; the second fills a buffer
    // of that size (8-byte aligned), and the list it holds is walked while
    // the buffer is alive.
    unsafe {
        GetNetworkParams(std::ptr::null_mut(), &mut len);
        if len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let info = buf.as_mut_ptr().cast::<FIXED_INFO_W2KSP1>();
        if GetNetworkParams(info, &mut len) != 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut node = &raw const (*info).DnsServerList;
        while !node.is_null() {
            // `CHAR`s: signed bytes of ASCII.
            let text: Vec<u8> = (*node)
                .IpAddress
                .String
                .iter()
                .map(|&c| c as u8)
                .take_while(|&b| b != 0)
                .collect();
            if let Some(sa) = std::str::from_utf8(&text).ok().and_then(parse_server) {
                out.push(sa);
            }
            node = (*node).Next;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A response with the question echoed and `answers` appended.
    fn response(id: u16, qname: &[u8], qtype: u16, rcode: u8, answers: &[Vec<u8>]) -> Vec<u8> {
        let mut r = id.to_be_bytes().to_vec();
        r.extend_from_slice(&[0x81, 0x80 | rcode]);
        r.extend_from_slice(&[0, 1]);
        r.extend_from_slice(&(answers.len() as u16).to_be_bytes());
        r.extend_from_slice(&[0, 0, 0, 0]);
        r.extend_from_slice(qname);
        r.extend_from_slice(&qtype.to_be_bytes());
        r.extend_from_slice(&[0, 1]);
        for a in answers {
            r.extend_from_slice(a);
        }
        r
    }

    /// An answer whose name is a pointer to the question (offset 12).
    fn rr(t: u16, ttl: u32, rdata: &[u8]) -> Vec<u8> {
        let mut a = vec![0xc0, 12];
        a.extend_from_slice(&t.to_be_bytes());
        a.extend_from_slice(&[0, 1]);
        a.extend_from_slice(&ttl.to_be_bytes());
        a.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        a.extend_from_slice(rdata);
        a
    }

    const QNAME: &[u8] = b"\x07example\x03com\x00";

    #[test]
    fn queries_are_built_and_bad_names_refused() {
        let q = build_query(7, "example.com.", 15).unwrap();
        assert_eq!(&q[..4], &[0, 7, 1, 0]);
        assert!(q.ends_with(b"\x07example\x03com\x00\x00\x0f\x00\x01"));
        assert_eq!(build_query(1, "a..b", 1), Err(DnsError::BadName));
        assert_eq!(build_query(1, &"a".repeat(64), 1), Err(DnsError::BadName));
        assert_eq!(
            reverse_name("1.2.3.4".parse().unwrap()),
            "4.3.2.1.in-addr.arpa"
        );
        assert!(reverse_name("::1".parse().unwrap()).starts_with("1.0.0.0."));
        assert!(reverse_name("::1".parse().unwrap()).ends_with(".ip6.arpa"));
    }

    #[test]
    fn records_are_read_in_the_shape_node_returns_them() {
        // MX with a compressed exchange name.
        let mut mx = vec![0, 10];
        mx.extend_from_slice(b"\x04mail\xc0\x0c");
        let r = response(9, QNAME, 15, 0, &[rr(15, 300, &mx)]);
        let got = parse_response(&r, 9, 15).unwrap().unwrap();
        assert_eq!(
            got,
            vec![
                json!({ "type": "MX", "ttl": 300, "data": { "priority": 10, "exchange": "mail.example.com" } })
            ]
        );
        // TXT with two chunks, SRV, SOA, CAA.
        let txt = rr(16, 5, b"\x03abc\x02de");
        let got = parse_response(&response(9, QNAME, 16, 0, &[txt]), 9, 16)
            .unwrap()
            .unwrap();
        assert_eq!(got[0]["data"], json!({ "entries": ["abc", "de"] }));
        let mut srv = vec![0, 1, 0, 2, 0x1f, 0x90];
        srv.extend_from_slice(b"\x03srv\xc0\x0c");
        let got = parse_response(&response(9, QNAME, 33, 0, &[rr(33, 1, &srv)]), 9, 33)
            .unwrap()
            .unwrap();
        assert_eq!(
            got[0]["data"],
            json!({ "priority": 1, "weight": 2, "port": 8080, "name": "srv.example.com" })
        );
        let mut soa = b"\x02ns\xc0\x0c\x04host\xc0\x0c".to_vec();
        for n in [1u32, 2, 3, 4, 5] {
            soa.extend_from_slice(&n.to_be_bytes());
        }
        let got = parse_response(&response(9, QNAME, 6, 0, &[rr(6, 1, &soa)]), 9, 6)
            .unwrap()
            .unwrap();
        assert_eq!(got[0]["data"]["nsname"], "ns.example.com");
        assert_eq!(got[0]["data"]["hostmaster"], "host.example.com");
        assert_eq!(got[0]["data"]["minttl"], 5);
        let caa = [&[0x80u8, 5][..], b"issue", b"ca.test"].concat();
        let got = parse_response(&response(9, QNAME, 257, 0, &[rr(257, 1, &caa)]), 9, 257)
            .unwrap()
            .unwrap();
        assert_eq!(got[0]["data"], json!({ "critical": 1, "issue": "ca.test" }));
        // A and AAAA; a CNAME in the answer is not an A record.
        let cname = rr(5, 1, b"\x01x\xc0\x0c");
        let a = rr(1, 60, &[10, 0, 0, 1]);
        let got = parse_response(&response(9, QNAME, 1, 0, &[cname, a]), 9, 1)
            .unwrap()
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["data"]["address"], "10.0.0.1");
        let aaaa = rr(28, 60, &Ipv6Addr::LOCALHOST.octets());
        let got = parse_response(&response(9, QNAME, 28, 0, &[aaaa]), 9, 28)
            .unwrap()
            .unwrap();
        assert_eq!(got[0]["data"]["address"], "::1");
        // ANY returns every type it can read.
        let any = response(
            9,
            QNAME,
            255,
            0,
            &[rr(1, 1, &[1, 1, 1, 1]), rr(16, 1, b"\x01t")],
        );
        assert_eq!(parse_response(&any, 9, 255).unwrap().unwrap().len(), 2);
    }

    #[test]
    fn answers_that_are_errors_map_to_c_ares_codes() {
        let id = 3;
        for (rcode, want) in [
            (2, DnsError::ServFail),
            (3, DnsError::NotFound),
            (4, DnsError::NotImp),
            (5, DnsError::Refused),
            (1, DnsError::FormErr),
        ] {
            assert_eq!(
                parse_response(&response(id, QNAME, 1, rcode, &[]), id, 1),
                Err(want)
            );
        }
        // No record of the type: ENODATA. Another id or a query: bad.
        assert_eq!(
            parse_response(&response(id, QNAME, 1, 0, &[]), id, 1),
            Err(DnsError::NoData)
        );
        assert_eq!(
            parse_response(&response(id + 1, QNAME, 1, 0, &[]), id, 1),
            Err(DnsError::BadResp)
        );
        let mut query = response(id, QNAME, 1, 0, &[]);
        query[2] = 0;
        assert_eq!(parse_response(&query, id, 1), Err(DnsError::BadResp));
        // A truncated message and a name that points to itself.
        let mut cut = response(id, QNAME, 1, 0, &[rr(1, 1, &[1, 2, 3, 4])]);
        cut.truncate(cut.len() - 3);
        assert_eq!(parse_response(&cut, id, 1), Err(DnsError::BadResp));
        let mut looped = response(id, QNAME, 1, 0, &[]);
        looped[12] = 0xc0;
        looped[13] = 12;
        assert_eq!(parse_response(&looped, id, 1), Err(DnsError::BadResp));
        // TC asks for TCP.
        let mut tc = response(id, QNAME, 1, 0, &[]);
        tc[2] |= 0x02;
        assert_eq!(parse_response(&tc, id, 1), Ok(None));
    }

    /// A server on the loopback answering MX for example.com over UDP, and
    /// the same question for `big.test` with TC so the client asks over TCP.
    #[test]
    fn a_server_is_asked_over_udp_and_tcp_when_the_answer_is_cut_short() {
        use std::net::TcpListener;
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = udp.local_addr().unwrap();
        let tcp = TcpListener::bind(addr).unwrap();
        let mut mx = vec![0, 5];
        mx.extend_from_slice(b"\x02mx\xc0\x0c");
        let mx_rr = rr(15, 60, &mx);
        let name_of = |q: &[u8]| q[12..q.len() - 4].to_vec();
        std::thread::spawn({
            let mx_rr = mx_rr.clone();
            move || {
                let mut buf = [0u8; 512];
                loop {
                    let Ok((n, from)) = udp.recv_from(&mut buf) else {
                        return;
                    };
                    let id = u16::from_be_bytes([buf[0], buf[1]]);
                    let qname = name_of(&buf[..n]);
                    let mut r = response(id, &qname, 15, 0, std::slice::from_ref(&mx_rr));
                    if qname.starts_with(b"\x03big") {
                        r = response(id, &qname, 15, 0, &[]);
                        r[2] |= 0x02;
                    } else if qname.starts_with(b"\x04nope") {
                        r = response(id, &qname, 15, 3, &[]);
                    }
                    let _ = udp.send_to(&r, from);
                }
            }
        });
        std::thread::spawn({
            move || {
                for stream in tcp.incoming().flatten() {
                    let mut s = stream;
                    let mut len = [0u8; 2];
                    if s.read_exact(&mut len).is_err() {
                        continue;
                    }
                    let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                    if s.read_exact(&mut q).is_err() {
                        continue;
                    }
                    let id = u16::from_be_bytes([q[0], q[1]]);
                    let qname = name_of(&q);
                    let r = response(id, &qname, 15, 0, std::slice::from_ref(&mx_rr));
                    let mut framed = (r.len() as u16).to_be_bytes().to_vec();
                    framed.extend_from_slice(&r);
                    let _ = s.write_all(&framed);
                }
            }
        });
        let got = resolve(&[addr], "example.com", 15).unwrap();
        assert_eq!(got[0]["data"]["exchange"], "mx.example.com");
        let got = resolve(&[addr], "big.test", 15).unwrap();
        assert_eq!(got[0]["data"]["priority"], 5);
        assert_eq!(resolve(&[addr], "nope.test", 15), Err(DnsError::NotFound));
        assert_eq!(resolve(&[], "example.com", 15), Err(DnsError::ConnRefused));
        // The next server is asked when the first does not answer.
        let dead: SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert!(resolve(&[dead, addr], "example.com", 15).is_ok());
    }

    #[test]
    fn servers_are_read_in_every_spelling() {
        assert_eq!(parse_server("1.2.3.4"), Some("1.2.3.4:53".parse().unwrap()));
        assert_eq!(
            parse_server("1.2.3.4:5353"),
            Some("1.2.3.4:5353".parse().unwrap())
        );
        assert_eq!(parse_server("::1"), Some("[::1]:53".parse().unwrap()));
        assert_eq!(
            parse_server("[::1]:5353"),
            Some("[::1]:5353".parse().unwrap())
        );
        assert_eq!(parse_server("not a server"), None);
        #[cfg(not(windows))]
        assert_eq!(
            servers_of_resolv_conf(
                "# c\nsearch x\nnameserver 8.8.8.8 # g\nnameserver fe80::1%eth0\nnameserver bad\n"
            ),
            vec![
                "8.8.8.8:53".parse().unwrap(),
                "[fe80::1]:53".parse().unwrap()
            ]
        );
    }
}
