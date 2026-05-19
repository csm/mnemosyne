use serde::{Deserialize, Serialize};

/// Rust representation of a Clojure value, mirroring the subset of
/// `cljrs_value::Value` variants that can be meaningfully serialized or
/// returned to callers outside the interpreter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ClojureValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Keyword(String),
    Symbol(String),
    List(Vec<ClojureValue>),
    Vector(Vec<ClojureValue>),
    Map(Vec<(ClojureValue, ClojureValue)>),
    Set(Vec<ClojureValue>),
    /// Opaque handle for values that don't have a structural Rust equivalent
    /// (functions, atoms, protocols, …). `tag` is the Clojure pr-str form.
    Opaque {
        tag: String,
    },
}

impl ClojureValue {
    pub fn is_truthy(&self) -> bool {
        !matches!(self, ClojureValue::Nil | ClojureValue::Bool(false))
    }
}

impl std::fmt::Display for ClojureValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s:?}"),
            Self::Keyword(k) => write!(f, ":{k}"),
            Self::Symbol(s) => write!(f, "{s}"),
            Self::List(items) => {
                write!(f, "(")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, ")")
            }
            Self::Vector(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Self::Map(pairs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k} {v}")?;
                }
                write!(f, "}}")
            }
            Self::Set(items) => {
                write!(f, "#{{")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "}}")
            }
            Self::Opaque { tag } => write!(f, "#<{tag}>"),
        }
    }
}

impl From<cljrs_value::Value> for ClojureValue {
    fn from(v: cljrs_value::Value) -> Self {
        use cljrs_value::Value as V;
        match v {
            V::Nil => Self::Nil,
            V::Bool(b) => Self::Bool(b),
            V::Long(n) => Self::Int(n),
            V::Double(d) => Self::Float(d),
            V::Char(c) => Self::String(c.to_string()),
            V::Str(ptr) => Self::String(ptr.get().clone()),
            V::Keyword(ptr) => Self::Keyword(ptr.get().full_name()),
            V::Symbol(ptr) => Self::Symbol(ptr.get().full_name()),
            V::List(ptr) => Self::List(ptr.get().iter().map(|v| Self::from(v.clone())).collect()),
            V::Vector(ptr) => {
                Self::Vector(ptr.get().iter().map(|v| Self::from(v.clone())).collect())
            }
            V::Map(map_val) => Self::Map(
                map_val
                    .iter()
                    .map(|(k, v)| (Self::from(k.clone()), Self::from(v.clone())))
                    .collect(),
            ),
            V::Set(set_val) => Self::Set(set_val.iter().map(|v| Self::from(v.clone())).collect()),
            // Strip metadata — the underlying value is what matters.
            V::WithMeta(val, _) => Self::from(*val),
            // Unwrap early-termination sentinel.
            V::Reduced(val) => Self::from(*val),
            // Everything else: fall back to the Clojure pr-str representation.
            other => Self::Opaque {
                tag: format!("{other}"),
            },
        }
    }
}
