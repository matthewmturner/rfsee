# rfsee

Search and view RFCs from the terminal.  A [TF-IDF](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) index is built on the contents of all RFCs from the [IETF](https://www.ietf.org/rfc/rfc-index.txt) and then saved locally in a JSON file.  A CLI app is provided for searching this index, with output that can be piped to other tools such as editors.

## Project Goals

I had two goals for this project at the offset.

1. Build something useful, that I hope to make use of in my day to day work
2. Build a high quality and performant project from scratch with only the minimal number of dependencies.

For point 2, I intend for the only dependency of this project to be a TLS library for making HTTPS calls (and potentially a JSON serialization library).  The initial release of this project will not have achieved that goal yet - but I plan to get there.

Some of the custom components that were created:

1. An HTTP client
2. A threadpool (largely copied from the Rust Book)
3. A TF-IDF index

And some of the planned components:

1. A text extractor / parser (currently using regex)
2. A custom serialization format (currently using simdjson)
3. An async runtime

I time boxed my work on this project to the spare hour or two I had each day (which I normally spend on work separate but related to my job) during the month of December 2024.  Moving forward I expect to delegate work on this project to a weekends / as I have time.

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
