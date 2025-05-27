import test from 'ava';

import * as path from 'path';
const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');
const CODE_DIR = path.join(REPO_DIR, 'd.py');

import * as codegraph from '../index.js';
const graph = new codegraph.CodeGraph('db');

test('resloving references', (t) => {
  graph.index(REPO_DIR, CODE_DIR);
  const nodes = graph.query("MATCH (n) RETURN *;");

  let defs: string[] = [];
  for (let n of nodes) {
    const name = path.basename(n.name);
    defs.push(`${name}(${n.startLine}:${n.endLine})`);
  }

  graph.clean();

  t.deepEqual(defs, ['d.py:D1(1:3)', 'd.py:D2(6:8)', 'd.py:D(11:12)'], 'unexpected definitions');
})
