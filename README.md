# rfsee

Search and view RFCs from the terminal.  A [TF-IDF](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) index is built on the contents of all RFCs from the [IETF](https://www.ietf.org/rfc/rfc-index.txt) and then saved locally in a JSON file.  A CLI app is provided for searching this index, with output that can be piped to other tools such as editors.

## Install

Currently the CLI app can only be installed with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

## Getting Started

After installing, you can run the following to create the index.

```bash
rfsee index
```

Workers fetch and tokenize each RFC as it arrives, then pass term frequencies through
a bounded queue for collection. Raw document text is released after tokenization.
Once all RFCs have been processed, the index computes corpus-wide inverse document
frequencies and final scores and saves the result.

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

## Contributing

This is a personal project that I am using to explore and learn to build an application with minimal dependencies - as such I will likely not be accepting outside contributions.  That being said bug reports are always welcome.
