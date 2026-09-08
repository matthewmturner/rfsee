# rfsee

Search and view RFCs from the terminal.  A [TF-IDF](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) index is built on the contents of all RFCs from the [IETF](https://www.ietf.org/rfc/rfc-index.txt) and then saved locally in a JSON file.  A CLI app is provided for searching this index, with output that can be piped to other tools such as editors.

## Install

Currently the CLI app can only be installed with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

## Getting Started

After installing, you can run the following to create the index.

```bash
rfsee index
```

The runtime's worker thread count defaults to the available parallelism of the machine. Override
it with the global `--parallelism` flag, for example `rfsee index --parallelism 4` or
`rfsee --parallelism 4 index`.

Then, to execute a query its as simple as 

```bash
rfsee search --terms MY_SEARCH_TERMS
```

Logging is controlled with repeatable `-v` flags. Logs are written to standard error so search
results can still be piped to another program.

- `-v` shows major stages such as loading, indexing, saving, and searching.
- `-vv` also shows timings, paths, counts, and periodic progress.
- `-vvv` also shows each RFC fetch or skip and per-term match counts.

The flag may appear before or after the subcommand, for example `rfsee -vv index` or
`rfsee search -vv --terms HTTP`.

## Index build design and memory profiling

The index build is buffered: all RFCs are fetched and held in memory before indexing
begins. The memory profiler offers two profiles of that current design:

| Profile | Data and execution path | Stages reported | Intended use |
| --- | --- | --- | --- |
| `synthetic` | Generates a deterministic in-process corpus with a configurable document count. It performs no network I/O and uses no worker pool. | `input`, `ingest`, `finish` | Repeatable comparisons between code changes. |
| `actual` | Downloads the live RFC corpus and indexes it through the production `par_load_rfcs_with_report` path and its worker pool. | `load_and_ingest`, `finish` | Validate real-world process memory, including downloads and parallel loading. Results depend on the live corpus, network outcomes, machine, and available parallelism. |

The production loading API collects all downloads and then ingests them before returning,
so the actual profile reports those operations together as `load_and_ingest`. The
synthetic profile controls those operations directly and can expose the finer boundary
between `input` and `ingest`:

```text
RFC index + RFC downloads
           │
           ▼
    Vec<RfcEntry>          input
           │
           ▼
 per-document term maps   ingest
           │
           ▼
 IDFs + searchable Index  finish
           │
           ▼
       index.json          not included in the memory profile
```

| Profile stage | Profiles | What is included | Memory state at the end |
| --- | --- | --- | --- |
| `input` | Synthetic | Generates the deterministic RFC corpus and materializes every `RfcEntry` in a `Vec`. | Every synthetic document's URL, title, and full text is live. |
| `ingest` | Synthetic | Passes each buffered entry to `add_rfc_entry`: text is tokenized, terms are counted, term frequencies are calculated, and the title and per-document term-frequency map are retained. Entries and their full text are dropped as the `Vec` is consumed. | The buffered source documents are gone; `processed_rfcs` and RFC details are live. |
| `load_and_ingest` | Actual | Fetches and parses the RFC index, downloads RFCs in parallel into a `Vec`, and then performs the same ingestion work described above. | The worker pool, `processed_rfcs`, RFC details, and load report are live; downloaded full text has been dropped. |
| `finish` | Both | Counts terms across documents, calculates [inverse-document frequencies](https://nlp.stanford.edu/IR-book/html/htmledition/inverse-document-frequency-1.html), generates per-term/per-document scores, and populates the searchable `Index`. | The completed index and the intermediate `TfIdf` working maps remain live through measurement, matching the application while it saves the index. |

Serialization to `index.json` is not included in either profile. Use
`just profile-build-index` to measure the complete CLI process, including serialization.
Network behavior and thread-pool overhead are included only in the actual profile.

Run either memory profile with:

```bash
RFSEE_BENCH_DOCS=100 just memory-profile synthetic
just memory-profile actual
```

`RFSEE_BENCH_DOCS` applies only to the synthetic profile. The actual profile always
attempts the complete live RFC corpus.

Each run appends one row per stage to `benches/memory-profile.csv`. The `profile` column
distinguishes synthetic and actual results. Stage timing uses a monotonic clock:
`start_elapsed_ms` and `end_elapsed_ms` are millisecond offsets from the beginning of the
run, and `duration_ms` is their difference. The memory columns have the following
meanings:

- `alloc_count`: allocations made during the stage.
- `retained_bytes`: signed change in live heap during the stage; it can be negative when
  the stage frees more memory than it retains.
- `peak_growth_bytes`: highest live heap during the stage minus live heap at stage start.
- `heap_start_bytes`, `heap_end_bytes`, and `heap_peak_bytes`: allocator-tracked live heap
  at the stage boundaries and high-water point.
- `run_peak_heap_bytes`: allocator-tracked high-water heap for the complete run.
- `peak_rss_kib`: operating-system process high-water RSS for the complete run. Compare
  this only across runs on the same system.

## Contributing

This is a personal project that I am using to explore and learn to build an application with minimal dependencies - as such I will likely not be accepting outside contributions.  That being said bug reports are always welcome.
