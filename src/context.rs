//! Datenkontext einer Sitzung. Die Nutzdaten (OData-Antworten, Listenstand,
//! Detail-Bearbeitung) liegen als serde_json-Werte vor. Screens lesen und
//! schreiben sie ueber JSON-Pointer wie `/detail/video/title`, ohne dass pro
//! Feld ein eigenes Struct noetig ist.

use serde_json::{Map, Value};

static NULL: Value = Value::Null;

#[derive(Debug, Clone)]
pub struct Context {
    root: Value,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            root: Value::Object(Map::new()),
        }
    }
}

impl Context {
    /// Wert unter dem Pointer. Fehlende Pfade liefern `null`.
    pub fn get(&self, ptr: &str) -> &Value {
        self.root.pointer(ptr).unwrap_or(&NULL)
    }

    pub fn get_mut(&mut self, ptr: &str) -> Option<&mut Value> {
        self.root.pointer_mut(ptr)
    }

    /// Setzt den Wert unter dem Pointer. Fehlende Objekte auf dem Weg werden angelegt.
    pub fn set(&mut self, ptr: &str, value: Value) {
        let keys: Vec<&str> = ptr.trim_start_matches('/').split('/').collect();
        let mut cur = &mut self.root;
        for (i, key) in keys.iter().enumerate() {
            if !cur.is_object() {
                *cur = Value::Object(Map::new());
            }
            let Value::Object(map) = cur else {
                unreachable!("oben als Objekt sichergestellt");
            };
            if i + 1 == keys.len() {
                map.insert((*key).to_string(), value);
                return;
            }
            cur = map
                .entry((*key).to_string())
                .or_insert_with(|| Value::Object(Map::new()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_path_is_null() {
        let ctx = Context::default();
        assert!(ctx.get("/detail/video/title").is_null());
    }

    #[test]
    fn set_creates_intermediate_objects() {
        let mut ctx = Context::default();
        ctx.set("/detail/video/title", json!("Film"));
        assert_eq!(ctx.get("/detail/video/title"), "Film");
        ctx.set("/detail/video/title", json!("Neu"));
        assert_eq!(ctx.get("/detail/video/title"), "Neu");
    }

    #[test]
    fn get_mut_updates_array_element() {
        let mut ctx = Context::default();
        ctx.set("/list/items", json!([{ "ID": "a" }, { "ID": "b" }]));
        *ctx.get_mut("/list/items/1").unwrap() = json!({ "ID": "c" });
        assert_eq!(ctx.get("/list/items/1/ID"), "c");
    }
}
