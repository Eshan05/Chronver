use crate::{Basis, Report, Source, Stream, http::Fetcher, watchlist::NpmDates};

pub(crate) fn fill(fetch: &impl Fetcher, source: &Source, stream: &Stream, report: &mut Report) {
    crate::github_dates::fill(fetch, source, stream, report);
    let Some(package) = stream.npm_date_package.as_deref() else {
        return;
    };
    if report.basis != Basis::Version
        || !report
            .notes
            .iter()
            .any(|n| n.authored_notes && n.date.is_none() && n.version.is_some())
    {
        return;
    }
    let Ok(document) = fetch.npm_dates(package) else {
        return;
    };
    let Ok(metadata) = serde_json::from_str::<NpmDates>(&document.body) else {
        return;
    };
    if metadata.name != package {
        return;
    }
    let mut filled = false;
    for note in &mut report.notes {
        if note.authored_notes
            && note.date.is_none()
            && let Some(version) = note.version.as_deref()
            && let Some(date) = metadata.dates.get(version)
        {
            note.date = Some(*date);
            filled = true;
        }
    }
    if filled {
        report.evidence.push(document.evidence);
    }
}
