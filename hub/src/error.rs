//! Refusals: the stable codes of spec/v1.md section 16, under the statuses of `trommi_wire::codes`.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub code: &'static str,
    pub message: String,
    /// further members of the answer, e.g. `voided: true` or `current` epoch
    pub extra: Option<Value>,
    pub retry_after: Option<u64>,
}

pub type Res<T> = Result<T, Refused>;

pub fn refuse(code: &'static str, message: impl Into<String>) -> Refused {
    Refused {
        code,
        message: message.into(),
        extra: None,
        retry_after: None,
    }
}

/// The status of a code (core/wire's table, which the clients read refusals by); a code outside it is a fault of
/// this hub, answered 500.
pub fn status_of(code: &str) -> u16 {
    trommi_wire::codes::status_of(code).unwrap_or(500)
}

impl Refused {
    pub fn with(mut self, extra: Value) -> Self {
        self.extra = Some(extra);
        self
    }
    pub fn retry(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds.max(1));
        self
    }
    pub fn status(&self) -> u16 {
        status_of(self.code)
    }
    pub fn body(&self) -> Value {
        let mut v = json!({ "error": self.code, "message": self.message });
        if let (Some(Value::Object(extra)), Value::Object(out)) = (&self.extra, &mut v) {
            for (k, x) in extra {
                out.insert(k.clone(), x.clone());
            }
        }
        // (the wait is in the `retry-after` header and here, for a client that reads only the body)
        if let Some(seconds) = self.retry_after {
            v["retry_after"] = json!(seconds);
        }
        v
    }
}

impl From<rusqlite::Error> for Refused {
    fn from(e: rusqlite::Error) -> Self {
        // The text of a database error never reaches a client: it is logged here, the answer is generic.
        crate::log::warn(
            "database_error",
            serde_json::json!({ "error": e.to_string() }),
        );
        Refused {
            code: "internal",
            message: "internal error".into(),
            extra: None,
            retry_after: None,
        }
    }
}

impl From<crate::observer::Refusal> for Refused {
    fn from(r: crate::observer::Refusal) -> Self {
        use crate::observer::Refusal::*;
        match r {
            BadCommit(m) => refuse("bad-commit", m),
            BadKeyPackage(m) => refuse("bad-key-package", m),
            BadFormat(m) => refuse("bad-format", m),
            Busy => refuse(
                "overloaded",
                "the state changed under this request: try again",
            )
            .retry(1),
        }
    }
}

impl From<trommi_wire::layout::Malformed> for Refused {
    fn from(m: trommi_wire::layout::Malformed) -> Self {
        refuse("bad-format", m.0)
    }
}
