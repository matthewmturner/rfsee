# rfsee

Search and view RFCs from the terminal.  A [TF-IDF](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) index is built on the contents of all RFCs from the [IETF](https://www.ietf.org/rfc/rfc-index.txt) and then saved locally in a JSON file.  A CLI app is provided for searching this index, with output that can be piped to other tools such as editors.

## Install

Currently the CLI app can only be installed with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

## Getting Started

After installing, you can run the following to create the index.

```bash
rfsee index
```

Then, to execute a query its as simple as 

```bash
rfsee search --terms MY_SEARCH_TERMS
```

## Contributing

This is a personal project that I am using to explore and learn to build an application with minimal dependencies - as such I will likely not be accepting outside contributions.  That being said bug reports are always welcome.
