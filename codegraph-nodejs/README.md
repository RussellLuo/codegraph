# CodeGraph Node.js Bindings

Node.js bindings for CodeGraph.


## Installation

```bash
npm install @codegraph-js/codegraph
```


## Quick Start

TODO:

```javascript
```


## Examples

- [Parse Python code](examples/parse_python_code.js)


## Development

Install [napi-rs][1]:

```bash
npm install -g @napi-rs/cli
```

Rename the package:

```bash
napi rename -n @org/pkg
```

Change the version:

```bash
npm version <newversion>
```

Build the native package:

```bash
napi build --platform --release
```

Run the examples:

```bash
node examples/parse_python_code.js
```


[1]: https://napi.rs/
