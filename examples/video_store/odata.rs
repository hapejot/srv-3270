//! Minimaler OData-V4-Client fuer den video-store Service (VideoSet).

use anyhow::{anyhow, bail, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::Value;
use std::time::Instant;
use tracing::{info, warn};

#[derive(Deserialize)]
struct ListResponse {
    #[serde(rename = "@odata.count", default)]
    count: Option<i64>,
    value: Vec<Value>,
}

/// Listenfilter, wie er im 3270-Befehl gesetzt wird.
#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub text: Option<String>,
    pub tag: Option<String>,
    pub only_unreviewed: bool,
}

impl Filter {
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(t) = &self.text {
            parts.push(format!("Text='{t}'"));
        }
        if let Some(t) = &self.tag {
            parts.push(format!("Tag='{t}'"));
        }
        if self.only_unreviewed {
            parts.push("nur ungesehen".to_string());
        }
        if parts.is_empty() {
            "keiner".to_string()
        } else {
            parts.join(", ")
        }
    }

    pub fn to_odata(&self) -> String {
        let mut parts = vec!["deleted_at eq null".to_string()];
        if let Some(t) = &self.text {
            let t = escape(t);
            parts.push(format!("(contains(name,'{t}') or contains(title,'{t}'))"));
        }
        if let Some(tag) = &self.tag {
            parts.push(format!(
                "_VideoTags/any(vt:vt/tag/name eq '{}')",
                escape(tag)
            ));
        }
        if self.only_unreviewed {
            parts.push("is_reviewed ne true".to_string());
        }
        parts.join(" and ")
    }
}

/// OData-String-Literal: einfache Anfuehrungszeichen verdoppeln.
pub fn escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[derive(Clone)]
pub struct Odata {
    http: reqwest::Client,
    base: String,
    user: String,
}

const DETAIL_EXPAND: &str = "_VideoTags($expand=tag($select=name))";
const LIST_SELECT: &str =
    "ID,IsActiveEntity,name,title,duration_seconds,width,height,codec,file_size,is_reviewed";

fn check_id(id: &str) -> Result<()> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        Ok(())
    } else {
        bail!("ungueltige ID")
    }
}

impl Odata {
    pub fn new(base: String, user: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            user,
        })
    }

    /// Baut die Anfrage, loggt Methode, vollstaendige URL, Status und Dauer und sendet sie.
    async fn send(&self, rb: RequestBuilder) -> Result<reqwest::Response> {
        let req = rb.build()?;
        let method = req.method().clone();
        let url = req.url().to_string();
        let shown = percent_encoding::percent_decode_str(&url)
            .decode_utf8_lossy()
            .into_owned();
        info!(%method, url = %shown, "odata request");
        let started = Instant::now();
        match self.http.execute(req).await {
            Ok(resp) => {
                info!(
                    %method,
                    url = %shown,
                    status = %resp.status(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "odata response"
                );
                Ok(resp)
            }
            Err(e) => {
                warn!(
                    %method,
                    url = %shown,
                    error = %e,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "odata request failed"
                );
                Err(e.into())
            }
        }
    }

    /// Liefert (Seite als JSON-Objekte, Gesamtanzahl).
    pub async fn list(
        &self,
        filter: &Filter,
        skip: usize,
        top: usize,
    ) -> Result<(Vec<Value>, Option<i64>)> {
        let skip = skip.to_string();
        let top = top.to_string();
        let filter = filter.to_odata();
        let rb = self
            .http
            .get(format!("{}/VideoSet", self.base))
            .header("X-Forwarded-User", &self.user)
            .query(&[
                ("$filter", filter.as_str()),
                ("$orderby", "name asc"),
                ("$skip", skip.as_str()),
                ("$top", top.as_str()),
                ("$count", "true"),
                ("$select", LIST_SELECT),
            ]);
        let resp = self.send(rb).await?.error_for_status()?;
        let body: ListResponse = serde_json::from_str(&resp.text().await?)?;
        Ok((body.value, body.count))
    }

    fn key_url(&self, id: &str) -> Result<String> {
        check_id(id)?;
        Ok(format!(
            "{}/VideoSet(ID={id},IsActiveEntity=true)",
            self.base
        ))
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        let rb = self
            .http
            .get(self.key_url(id)?)
            .header("X-Forwarded-User", &self.user)
            .query(&[("$expand", DETAIL_EXPAND)]);
        let resp = self.send(rb).await?.error_for_status()?;
        Ok(serde_json::from_str(&resp.text().await?)?)
    }

    pub async fn patch(&self, id: &str, title: &str, reviewed: bool) -> Result<()> {
        let body = serde_json::json!({ "title": title, "is_reviewed": reviewed });
        let rb = self
            .http
            .patch(self.key_url(id)?)
            .header("X-Forwarded-User", &self.user)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?);
        let resp = self.send(rb).await?;
        if !resp.status().is_success() {
            return Err(anyhow!("PATCH fehlgeschlagen: HTTP {}", resp.status()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_escapes_quotes() {
        let f = Filter {
            text: Some("o'neil".into()),
            ..Default::default()
        };
        assert!(f.to_odata().contains("contains(name,'o''neil')"));
    }

    #[test]
    fn default_filter_hides_deleted() {
        assert_eq!(Filter::default().to_odata(), "deleted_at eq null");
    }

    #[test]
    fn id_guard() {
        assert!(check_id("40ee386b-35e9-4490-93d2-929faef5cc9b").is_ok());
        assert!(check_id("x)or(1").is_err());
    }
}
