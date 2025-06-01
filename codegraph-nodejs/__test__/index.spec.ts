import test from 'ava';

import * as path from 'path';
const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');

import * as codegraph from '../index.js';

const config: codegraph.ParserConfig = {
  // Only include "d.py"
  ignorePatterns: [
    "*",
    "!d.py",
  ],
};
const parser = new codegraph.Parser(config);

test('parsing nodes', (t) => {
  const { nodes, relationships } = parser.parse(REPO_DIR);

  let defs: string[] = [];
  for (let n of nodes) {
    const name = path.basename(n.name);
    defs.push(`${name}(${n.startLine}:${n.endLine})`);
  }

  let rels: string[] = [];
  for (let r of relationships ) {
    rels.push(`${r.type} (${r.from.name} => ${r.to.name})`);
  }

  t.deepEqual(defs, ['d.py:D1(1:3)', 'd.py:D2(6:8)', 'd.py:D(11:12)'], 'unexpected definitions');
  t.deepEqual(rels, ['Contains (d.py => d.py:D1)', 'Contains (d.py => d.py)', 'Contains (d.py => d.py:D)'], 'unexpected relationships');
})