# Synthetic Source Fixtures

`github-releases.json` uses invented releases in the GitHub API response shape.
It exercises range boundaries, CRLF Markdown, links, and preservation of the
authored body without using text from a real publisher.

Other tests use deliberately small synthetic documents to isolate parser,
pagination, range, cache, and failure behavior. Full live reports are generated
under ignored `artifacts/live/` by `cargo run --example live_smoke`.
