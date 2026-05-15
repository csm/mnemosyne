use serde::{Deserialize, Serialize};

/// Rust representation of a Clojure value.
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
    // Opaque handle for values that can't be serialized (e.g. functions, atoms)
    Opaque { tag: String },
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
            Self::Bool(b) => write!(f, "{}", b),
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(n) => write!(f, "{}", n),
            Self::String(s) => write!(f, "{:?}", s),
            Self::Keyword(k) => write!(f, ":{}", k),
            Self::Symbol(s) => write!(f, "{}", s),
            Self::List(items) => {
                write!(f, "(")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 { write!(f, " ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
            Self::Vector(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 { write!(f, " ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::Map(pairs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{} {}", k, v)?;
                }
                write!(f, "}}")
            }
            Self::Set(items) => {
                write!(f, "#{{")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 { write!(f, " ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Self::Opaque { tag } => write!(f, "#<{}>", tag),
        }
    }
}
