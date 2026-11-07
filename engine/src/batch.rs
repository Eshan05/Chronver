//! Bounded, read-only agent queries. Retrieval and response truncation are separate contracts.
use crate::{Channel, Engine, Error, HistoryCursor, Report, Result, Selection, http::Fetcher};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::Read,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

pub const MAX_INPUT_BYTES: u64 = 1024 * 1024;
pub const MAX_QUERIES: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub queries: Vec<Query>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub key: String,
    pub package: String,
    #[serde(default)]
    pub component: Option<String>,
    pub query: QueryKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryKind {
    Range {
        from: String,
        to: String,
        channel: Option<Channel>,
    },
    Updates {
        since: NaiveDate,
        until: NaiveDate,
    },
    History {
        cursor: Option<HistoryCursor>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub note_offset: usize,
    pub note_limit: usize,
    pub body_offset_bytes: usize,
    pub body_bytes: usize,
    pub body_budget_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            note_offset: 0,
            note_limit: 20,
            body_offset_bytes: 0,
            body_bytes: 16_384,
            body_budget_bytes: 65_536,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<()> {
        if !(1..=100).contains(&self.note_limit)
            || !(4..=65_536).contains(&self.body_bytes)
            || !(4..=1_048_576).contains(&self.body_budget_bytes)
        {
            return Err(Error::Query(
                "note-limit must be 1..100, body-bytes 4..65536 and body-budget-bytes 4..1048576"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub key: String,
    pub package: String,
    pub elapsed_ms: u128,
    pub report: Option<Value>,
    pub next: Option<HistoryCursor>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub schema_version: u32,
    pub jobs: usize,
    pub elapsed_ms: u128,
    pub complete: bool,
    pub output_complete: bool,
    pub body_bytes_returned: usize,
    pub results: Vec<Item>,
}
impl Response {
    pub fn failed(&self) -> bool {
        self.results.iter().any(|r| {
            r.error.is_some()
                || r.report
                    .as_ref()
                    .is_some_and(|r| r["status"] == "source_failed")
        })
    }
}

pub fn read_request(reader: impl Read) -> Result<Request> {
    let mut bytes = Vec::new();
    reader.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(Error::Query("batch input exceeds 1 MiB".into()));
    }
    // Accept the UTF-8 BOM commonly emitted by Windows PowerShell.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    Ok(serde_json::from_slice(bytes)?)
}

fn retrieve<F: Fetcher>(
    engine: &Engine<F>,
    q: &Query,
    include_inactive: bool,
) -> Result<(Report, Option<HistoryCursor>)> {
    let selection = match &q.query {
        QueryKind::History { cursor } => {
            let page = engine.history(
                &q.package,
                q.component.as_deref(),
                cursor.clone(),
                include_inactive,
            )?;
            return Ok((page.report, page.next));
        }
        QueryKind::Range { from, to, channel } => {
            let p = engine.package(&q.package)?;
            let s = engine.stream(p, q.component.as_deref())?;
            Selection::Versions {
                from: from.clone(),
                to: to.clone(),
                channel: channel.unwrap_or(s.channel),
            }
        }
        QueryKind::Updates { since, until } => Selection::Dates {
            since: *since,
            until: *until,
        },
    };
    engine
        .notes(
            &q.package,
            q.component.as_deref(),
            selection,
            include_inactive,
        )
        .map(|r| (r, None))
}

pub fn run<F: Fetcher + Sync>(
    engine: &Engine<F>,
    request: Request,
    jobs: usize,
    limits: Limits,
    include_inactive: bool,
) -> Result<Response> {
    limits.validate()?;
    if request.schema_version != 1
        || request.queries.is_empty()
        || request.queries.len() > MAX_QUERIES
    {
        return Err(Error::Query(
            "expected schema_version 1 and 1..32 queries".into(),
        ));
    }
    if !(1..=8).contains(&jobs) {
        return Err(Error::Query("jobs must be 1..8".into()));
    }
    let mut keys = HashSet::new();
    for q in &request.queries {
        if q.key.trim().is_empty() || q.key.len() > 128 || !keys.insert(&q.key) {
            return Err(Error::Query(
                "batch keys must be unique, nonempty and at most 128 UTF-8 bytes".into(),
            ));
        }
    }
    let started = Instant::now();
    let jobs = jobs.min(request.queries.len());
    let next = AtomicUsize::new(0);
    let retrieved = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(|| {
                    let mut rows = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(q) = request.queries.get(i) else {
                            break;
                        };
                        let started = Instant::now();
                        let result = retrieve(engine, q, include_inactive);
                        rows.push((i, started.elapsed().as_millis(), result));
                    }
                    rows
                })
            })
            .collect();
        let mut rows = Vec::new();
        for handle in handles {
            rows.extend(
                handle
                    .join()
                    .map_err(|_| Error::Source("batch worker panicked".into()))?,
            );
        }
        Ok::<_, Error>(rows)
    })?;
    let mut retrieved = retrieved;
    retrieved.sort_by_key(|r| r.0);
    let mut remaining = limits.body_budget_bytes;
    let mut results = Vec::new();
    for (i, elapsed_ms, result) in retrieved {
        let q = &request.queries[i];
        let (report, next, error) = match result {
            Ok((report, next)) => (
                Some(bound_report(report, limits, &mut remaining)?),
                next,
                None,
            ),
            Err(error) => (None, None, Some(error.to_string())),
        };
        results.push(Item {
            key: q.key.clone(),
            package: q.package.clone(),
            elapsed_ms,
            report,
            next,
            error,
        });
    }
    Ok(Response {
        schema_version: 1,
        jobs,
        elapsed_ms: started.elapsed().as_millis(),
        complete: results
            .iter()
            .all(|r| r.error.is_none() && r.report.as_ref().is_some_and(|r| r["complete"] == true)),
        output_complete: results.iter().all(|r| {
            r.report
                .as_ref()
                .is_none_or(|r| r["output_complete"] == true)
        }),
        body_bytes_returned: limits.body_budget_bytes - remaining,
        results,
    })
}

fn bound_report(report: Report, limits: Limits, remaining: &mut usize) -> Result<Value> {
    let source_complete = report.complete;
    let total = report.notes.len();
    let offset = limits.note_offset.min(total);
    let mut value = serde_json::to_value(report)?;
    let notes = value["notes"]
        .as_array_mut()
        .expect("serialized report notes");
    let end = offset.saturating_add(limits.note_limit).min(total);
    *notes = notes.drain(offset..end).collect();
    let mut output_complete = offset == 0 && end == total;
    for note in &mut *notes {
        let original = note["body"].as_str().expect("serialized note body");
        let total_bytes = original.len();
        let mut start = limits.body_offset_bytes.min(total_bytes);
        while !original.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = start
            .saturating_add(limits.body_bytes.min(*remaining))
            .min(total_bytes);
        while !original.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = start != 0 || end != total_bytes;
        let body = original[start..end].to_owned();
        *remaining -= body.len();
        note["body"] = json!(body);
        note["body_truncated"] = json!(truncated);
        note["body_range"] = json!({"start_byte":start,"end_byte":end,"total_bytes":total_bytes,"next_offset_bytes":(end < total_bytes).then_some(end)});
        output_complete &= !truncated;
    }
    let count = notes.len();
    value["source_complete"] = json!(source_complete);
    value["output_complete"] = json!(output_complete);
    value["complete"] = json!(source_complete && output_complete);
    value["output"] = json!({"total_notes":total,"note_offset":offset,"returned_notes":count,"next_note_offset":(end < total).then_some(end),"body_offset_bytes":limits.body_offset_bytes,"body_bytes":limits.body_bytes});
    if !output_complete {
        value["warnings"].as_array_mut().expect("serialized report warnings").push(json!("Output is a bounded excerpt, not complete notes. Follow next_note_offset/body_range offsets with adequate body budget; never treat omitted text as absent upstream."));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Basis, Note};

    fn report() -> Report {
        Report {
            package: "test".into(),
            stream: "main".into(),
            scope: "Test".into(),
            basis: Basis::Version,
            status: "available".into(),
            complete: true,
            notes: (0..3)
                .map(|i| Note {
                    title: format!("Release {i}"),
                    version: Some(format!("1.{i}.0")),
                    date: None,
                    url: "https://example.com/notes".into(),
                    body: "Fixed \u{1f680} Unicode rendering and a real regression.".into(),
                    authored_notes: true,
                    preview: false,
                })
                .collect(),
            warnings: vec!["Material scope caveat".into()],
            evidence: vec![],
            attempts: vec![],
        }
    }

    #[test]
    fn excerpts_cannot_claim_full_notes_and_have_utf8_safe_continuations() {
        let original = report().notes[0].body.clone();
        let mut offset = 0;
        let mut recovered = String::new();
        loop {
            let limits = Limits {
                note_limit: 1,
                body_bytes: 7,
                body_offset_bytes: offset,
                ..Limits::default()
            };
            let mut budget = 100;
            let value = bound_report(report(), limits, &mut budget).unwrap();
            assert_eq!(value["source_complete"], true);
            assert_eq!(value["complete"], false);
            assert_eq!(value["output_complete"], false);
            assert_eq!(value["output"]["next_note_offset"], 1);
            assert!(
                value["warnings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "Material scope caveat")
            );
            let note = &value["notes"][0];
            let body = note["body"].as_str().unwrap();
            assert!(body.len() <= 7);
            recovered.push_str(body);
            let Some(next) = note["body_range"]["next_offset_bytes"].as_u64() else {
                break;
            };
            assert!(next as usize > offset);
            offset = next as usize;
        }
        assert_eq!(recovered, original);
    }

    #[test]
    fn whole_batch_body_budget_is_hard_and_metadata_is_not_discarded() {
        let mut budget = 12;
        let value = bound_report(report(), Limits::default(), &mut budget).unwrap();
        let bytes: usize = value["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["body"].as_str().unwrap().len())
            .sum();
        assert!(bytes <= 12);
        assert_eq!(12 - bytes, budget);
        assert_eq!(value["output_complete"], false);
        assert_eq!(value["notes"].as_array().unwrap().len(), 3);
        assert!(value["notes"][2]["body_truncated"].as_bool().unwrap());
        let mut budget = 1000;
        let value = bound_report(report(), Limits::default(), &mut budget).unwrap();
        assert_eq!(value["complete"], true);
        assert_eq!(value["output_complete"], true);
        assert!(value["output"]["next_note_offset"].is_null());
    }

    #[test]
    fn note_offsets_do_not_masquerade_as_empty_complete_history() {
        let mut budget = 1000;
        let value = bound_report(
            report(),
            Limits {
                note_offset: 99,
                ..Limits::default()
            },
            &mut budget,
        )
        .unwrap();
        assert_eq!(value["output"]["total_notes"], 3);
        assert_eq!(value["output"]["note_offset"], 3);
        assert_eq!(value["complete"], false);
        assert!(value["notes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn input_bounds_and_unknown_fields_are_enforced() {
        assert!(read_request(&vec![b' '; MAX_INPUT_BYTES as usize + 1][..]).is_err());
        assert!(
            read_request(br#"{"schema_version":1,"queries":[],"unexpected":true}"#.as_slice())
                .is_err()
        );
        assert!(
            read_request(b"\xef\xbb\xbf{\"schema_version\":1,\"queries\":[]}".as_slice()).is_ok()
        );
    }
}
