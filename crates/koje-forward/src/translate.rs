//! Translating portal object paths between the context bus and the host bus
//! (M2.10): portal clients expect their request and session objects at
//! `/org/freedesktop/portal/desktop/{request,session}/<own bus name>/<token>`,
//! but on the host the portal builds them from the forwarder's host connection
//! name. The forwarder swaps that segment in object paths and inside bodies.

use zbus::message::Body;
use zbus::zvariant::{
    self, Array, Dict, ObjectPath, Signature, Str, Structure, StructureBuilder, Value,
};

const PORTAL: &str = "/org/freedesktop/portal/desktop/";
const KINDS: [&str; 2] = ["request", "session"];

/// `:1.20` → `1_20`, the segment portal clients put into their paths.
pub fn segment(unique_name: &str) -> String {
    unique_name.trim_start_matches(':').replace('.', "_")
}

/// Replaces one name segment by another in portal request/session paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    from: String,
    to: String,
}

impl PathMap {
    pub fn new(from_unique: &str, to_unique: &str) -> Self {
        Self {
            from: segment(from_unique),
            to: segment(to_unique),
        }
    }

    /// The translated path, if `s` is a portal path with the `from` segment.
    pub fn apply(&self, s: &str) -> Option<String> {
        let rest = s.strip_prefix(PORTAL)?;
        KINDS.iter().find_map(|kind| {
            let tail = rest.strip_prefix(kind)?.strip_prefix('/')?;
            let after = tail.strip_prefix(self.from.as_str())?;
            (after.is_empty() || after.starts_with('/'))
                .then(|| format!("{PORTAL}{kind}/{}{after}", self.to))
        })
    }

    pub fn path(&self, s: &str) -> String {
        self.apply(s).unwrap_or_else(|| s.to_owned())
    }
}

/// A copy of `v` with every string and object path translated.
pub fn rewrite(v: &Value<'_>, map: &PathMap) -> zvariant::Result<Value<'static>> {
    Ok(match v {
        Value::Str(s) => Value::Str(Str::from(map.path(s.as_str()))),
        Value::ObjectPath(p) => {
            Value::ObjectPath(ObjectPath::from_string_unchecked(map.path(p.as_str())))
        }
        Value::Value(inner) => Value::Value(Box::new(rewrite(inner, map)?)),
        Value::Array(a) => {
            let mut out = Array::new(a.element_signature());
            for e in a.inner() {
                out.append(rewrite(e, map)?)?;
            }
            Value::Array(out)
        }
        Value::Dict(d) => {
            let Signature::Dict { key, value } = d.signature() else {
                return Ok(v.try_to_owned()?.into());
            };
            let mut out = Dict::new(key, value);
            for (k, val) in d.iter() {
                out.append(rewrite(k, map)?, rewrite(val, map)?)?;
            }
            Value::Dict(out)
        }
        Value::Structure(s) => Value::Structure(rewrite_fields(s, map)?),
        // Numbers, signatures, fds, maybes: copied (fds are duplicated).
        other => other.try_to_owned()?.into(),
    })
}

fn rewrite_fields(s: &Structure<'_>, map: &PathMap) -> zvariant::Result<Structure<'static>> {
    let mut b = StructureBuilder::new();
    for f in s.fields() {
        b = b.append_field(rewrite(f, map)?);
    }
    b.build()
}

/// The body of a message as a structure with portal paths translated, ready
/// for `Builder::build` (zbus writes a structure body as its fields, under the
/// original signature; fds are duplicated and attached). `None` for no body.
pub fn rewrite_body(body: &Body, map: &PathMap) -> zbus::Result<Option<Structure<'static>>> {
    if matches!(body.signature(), Signature::Unit) {
        return Ok(None);
    }
    let fields: Structure<'_> = body.deserialize()?;
    Ok(Some(rewrite_fields(&fields, map)?))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn segments_and_paths() {
        assert_eq!(segment(":1.20"), "1_20");
        let to_host = PathMap::new(":1.47", ":1.6004");
        let p = "/org/freedesktop/portal/desktop/request/1_47/webrtc1";
        assert_eq!(
            to_host.path(p),
            "/org/freedesktop/portal/desktop/request/1_6004/webrtc1"
        );
        assert_eq!(
            to_host.path("/org/freedesktop/portal/desktop/session/1_47/s1"),
            "/org/freedesktop/portal/desktop/session/1_6004/s1"
        );
        // Other clients, other objects and lookalikes stay unchanged.
        for unchanged in [
            "/org/freedesktop/portal/desktop/request/1_470/x",
            "/org/freedesktop/portal/desktop/request/1_4/x",
            "/org/freedesktop/portal/desktop",
            "/org/freedesktop/Notifications",
            "dark",
        ] {
            assert_eq!(to_host.path(unchanged), unchanged);
        }
    }

    #[test]
    fn rewrites_nested_values() {
        let map = PathMap::new(":1.6004", ":1.47");
        let results: HashMap<&str, Value<'_>> = HashMap::from([
            (
                "session_handle",
                Value::from("/org/freedesktop/portal/desktop/session/1_6004/s1"),
            ),
            ("streams", Value::from(7u32)),
        ]);
        let v = Value::from(
            StructureBuilder::new()
                .add_field(0u32)
                .add_field(results)
                .build()
                .unwrap(),
        );
        let out = rewrite(&v, &map).unwrap();
        let Value::Structure(s) = out else {
            panic!("structure")
        };
        let Value::Dict(d) = &s.fields()[1] else {
            panic!("dict")
        };
        let handle: String = d.get::<&str, String>(&"session_handle").unwrap().unwrap();
        assert_eq!(handle, "/org/freedesktop/portal/desktop/session/1_47/s1");
        assert_eq!(d.get::<&str, u32>(&"streams").unwrap(), Some(7));
    }
}
