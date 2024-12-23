# rfsee

Search and view RFCs in Neovim and from the terminal.  A [TF-IDF](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) index is built on the contents of all RFCs from the [IETF](https://www.ietf.org/rfc/rfc-index.txt) and then saved locally in a JSON file.  A CLI app and NeoVim plugin are provided for searching this index.

## Install

### Terminal

Currently the CLI app can only be installed with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html).

### NeoVim

With Lazy

```lua
{
    'matthewmturner/rfsee',
    opts = {}
}
```

## Getting Started


After installing, you can run the following to create the index.

### Terminal

```bash
rfsee index
```

Then, to execute a query its as simple as 

```bash
rfsee search --terms MY_SEARCH_TERMS
```

### NeoVim

```vim
:RFCIndex
```

Then, to execute a query its as simple as 

```vim
:RFC MY_SEARCH_TERMS
```

The above will open a new buffer with the results from your search.  You can navigate up and down and then press `<Enter>` on a line to open that RFC in your browser.  In the future this will open the selected RFC in NeoVim.
