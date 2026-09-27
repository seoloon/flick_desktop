//! Owned Rust mirror of `mpv_node`.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char};

use serde::Serialize;

use crate::sys::{self, mpv_node};

/// A decoded mpv value. Maps keep mpv's key order irrelevant (sorted), which
/// is fine: no mpv property relies on map ordering.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Node {
    None,
    String(String),
    Flag(bool),
    Int64(i64),
    Double(f64),
    Array(Vec<Node>),
    Map(BTreeMap<String, Node>),
    Bytes(Vec<u8>),
}

impl Node {
    /// Deep-copies a C node.
    ///
    /// # Safety
    /// `node` must be a valid, initialised `mpv_node` as produced by libmpv.
    pub(crate) unsafe fn from_raw(node: &mpv_node) -> Self {
        // SAFETY: the caller guarantees `node` is valid, so the union member
        // selected by `format` is the initialised one and every pointer it
        // holds is valid for the advertised length.
        unsafe {
            match node.format {
                sys::MPV_FORMAT_STRING => Self::String(cstr_lossy(node.u.string)),
                sys::MPV_FORMAT_FLAG => Self::Flag(node.u.flag != 0),
                sys::MPV_FORMAT_INT64 => Self::Int64(node.u.int64),
                sys::MPV_FORMAT_DOUBLE => Self::Double(node.u.double_),
                sys::MPV_FORMAT_NODE_ARRAY => {
                    let list = &*node.u.list;
                    let values = raw_slice(list.values, list.num);
                    Self::Array(values.iter().map(|v| Self::from_raw(v)).collect())
                }
                sys::MPV_FORMAT_NODE_MAP => {
                    let list = &*node.u.list;
                    let values = raw_slice(list.values, list.num);
                    let keys = raw_slice(list.keys, list.num);
                    Self::Map(
                        keys.iter()
                            .zip(values)
                            .map(|(k, v)| (cstr_lossy(*k), Self::from_raw(v)))
                            .collect(),
                    )
                }
                sys::MPV_FORMAT_BYTE_ARRAY => {
                    let ba = &*node.u.ba;
                    let bytes = if ba.data.is_null() {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(ba.data.cast::<u8>(), ba.size)
                    };
                    Self::Bytes(bytes.to_vec())
                }
                _ => Self::None,
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Self::Map(m) => m.get(key),
            _ => None,
        }
    }

    /// Follows a `/`-separated path through nested maps, e.g. `"video-params/gamma"`.
    pub fn path(&self, path: &str) -> Option<&Node> {
        path.split('/').try_fold(self, |node, key| node.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int64(v) => Some(*v),
            Self::Double(v) if v.fract() == 0.0 => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            Self::Int64(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Flag(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Node]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn into_json(self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl From<&str> for Node {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}
impl From<String> for Node {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}
impl From<bool> for Node {
    fn from(b: bool) -> Self {
        Self::Flag(b)
    }
}
impl From<i64> for Node {
    fn from(v: i64) -> Self {
        Self::Int64(v)
    }
}
impl From<f64> for Node {
    fn from(v: f64) -> Self {
        Self::Double(v)
    }
}

/// A C-compatible node tree built from a [`Node`], kept alive for the duration
/// of an FFI call. All allocations are owned by this struct.
pub(crate) struct OwnedCNode {
    pub root: mpv_node,
    _strings: Vec<CString>,
    // Boxed on purpose: nodes point at these lists, so their addresses must
    // stay stable while the Vec grows.
    #[allow(clippy::vec_box)]
    _lists: Vec<Box<sys::mpv_node_list>>,
    _values: Vec<Vec<mpv_node>>,
    _keys: Vec<Vec<*mut c_char>>,
}

impl OwnedCNode {
    pub fn new(node: &Node) -> Result<Self, crate::Error> {
        let mut this = Self {
            root: mpv_node::empty(),
            _strings: Vec::new(),
            _lists: Vec::new(),
            _values: Vec::new(),
            _keys: Vec::new(),
        };
        this.root = this.build(node)?;
        Ok(this)
    }

    fn cstring(&mut self, s: &str) -> Result<*mut c_char, crate::Error> {
        let c = CString::new(s).map_err(|_| crate::Error::Nul(s.to_owned()))?;
        // The CString's heap buffer does not move when the CString itself is
        // moved into the Vec, so the pointer stays valid.
        let ptr = c.as_ptr().cast_mut();
        self._strings.push(c);
        Ok(ptr)
    }

    fn build(&mut self, node: &Node) -> Result<mpv_node, crate::Error> {
        let mut out = mpv_node::empty();
        match node {
            Node::None => {}
            Node::String(s) => {
                out.format = sys::MPV_FORMAT_STRING;
                out.u.string = self.cstring(s)?;
            }
            Node::Flag(b) => {
                out.format = sys::MPV_FORMAT_FLAG;
                out.u.flag = i32::from(*b);
            }
            Node::Int64(v) => {
                out.format = sys::MPV_FORMAT_INT64;
                out.u.int64 = *v;
            }
            Node::Double(v) => {
                out.format = sys::MPV_FORMAT_DOUBLE;
                out.u.double_ = *v;
            }
            Node::Array(items) => {
                let mut values = items.iter().map(|n| self.build(n)).collect::<Result<Vec<_>, _>>()?;
                out.format = sys::MPV_FORMAT_NODE_ARRAY;
                out.u.list = self.list(&mut values, None);
                self._values.push(values);
            }
            Node::Map(map) => {
                let mut values = Vec::with_capacity(map.len());
                let mut keys = Vec::with_capacity(map.len());
                for (k, v) in map {
                    keys.push(self.cstring(k)?);
                    values.push(self.build(v)?);
                }
                out.format = sys::MPV_FORMAT_NODE_MAP;
                out.u.list = self.list(&mut values, Some(&mut keys));
                self._values.push(values);
                self._keys.push(keys);
            }
            Node::Bytes(_) => return Err(crate::Error::Format("byte arrays are not sent to mpv".into())),
        }
        Ok(out)
    }

    fn list(&mut self, values: &mut [mpv_node], keys: Option<&mut Vec<*mut c_char>>) -> *mut sys::mpv_node_list {
        let mut list = Box::new(sys::mpv_node_list {
            num: values.len() as i32,
            values: values.as_mut_ptr(),
            keys: keys.map_or(std::ptr::null_mut(), |k| k.as_mut_ptr()),
        });
        let ptr: *mut sys::mpv_node_list = &mut *list;
        self._lists.push(list);
        ptr
    }
}

/// # Safety
/// `ptr` must be null or a valid NUL-terminated string.
pub(crate) unsafe fn cstr_lossy(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: guaranteed by the caller.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// # Safety
/// `ptr` must be valid for `len` elements (or `len <= 0`).
unsafe fn raw_slice<'a, T>(ptr: *const T, len: i32) -> &'a [T] {
    if ptr.is_null() || len <= 0 {
        &[]
    } else {
        // SAFETY: guaranteed by the caller.
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_c_representation() {
        let node = Node::Map(BTreeMap::from([
            ("a".to_owned(), Node::Int64(3)),
            ("b".to_owned(), Node::Array(vec![Node::from("x"), Node::Flag(true), Node::Double(1.5)])),
        ]));
        let c = OwnedCNode::new(&node).unwrap();
        // SAFETY: `c.root` is a valid node tree owned by `c`.
        let back = unsafe { Node::from_raw(&c.root) };
        assert_eq!(back, node);
    }

    #[test]
    fn path_lookup() {
        let node = Node::Map(BTreeMap::from([(
            "video-params".to_owned(),
            Node::Map(BTreeMap::from([("gamma".to_owned(), Node::from("pq"))])),
        )]));
        assert_eq!(node.path("video-params/gamma").and_then(Node::as_str), Some("pq"));
        assert!(node.path("video-params/missing").is_none());
    }
}
