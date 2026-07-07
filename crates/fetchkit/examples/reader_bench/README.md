# FetchKit Reader Benchmarks

This example is a Mira study that compares reader output from real internet
fetches against fixed public web pages.

## Pages

- `https://www.rust-lang.org/learn/`
- `https://docs.rs/reqwest/latest/reqwest/`
- `https://en.wikipedia.org/wiki/Rust_(programming_language)`
- `https://github.com/everruns/fetchkit`
- `https://stackoverflow.com/questions/419163/what-does-if-name-main-do`

## Run

Install the Mira host:

```bash
brew install everruns/tap/mira
```

List the study:

```bash
mira --example reader_bench list
```

Run available targets:

```bash
mira --example reader_bench run
```

Write a report:

```bash
mira --example reader_bench run --format html --out target/fetchkit-bench.html
```

## Targets

- `fetchkit`: always available; fetches the public URL with FetchKit.
- `jina-reader`: always available; fetches the public URL through Jina Reader.
- `firecrawl`: available when `FIRECRAWL_API_KEY` is set.
- `trafilatura-readability`: available when `trafilatura` is on `PATH`, or when
  `FETCHKIT_BENCH_TRAF_CMD` is set to a shell command containing `{url}`.
- `crawl4ai`: available when `crawl4ai` is on `PATH`, or when
  `FETCHKIT_BENCH_CRAWL4AI_CMD` is set to a shell command containing `{url}`.

Command template examples:

```bash
FETCHKIT_BENCH_TRAF_CMD='trafilatura -u {url} --markdown' mira --example reader_bench run
FETCHKIT_BENCH_CRAWL4AI_CMD='crawl4ai {url}' mira --example reader_bench run
```

## Metrics

- `token_count`: whitespace-token approximation for output size.
- `link_preservation`: expected URL markers preserved in the output.
- `main_content_precision`: required main-content markers divided by required
  markers plus detected boilerplate markers.
- `table_retention`: expected markdown table markers preserved.
- `code_retention`: expected fenced-code or inline-code markers preserved.
- `metadata_score`: title and metadata markers preserved.
