import test from 'ava';

import * as path from 'path';
const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');
const CODE_DIR = path.join(REPO_DIR, 'd.py');

import * as codegraph from '../index.js';
const parser = new codegraph.Parser();

test('resloving references', (t) => {
  const nodes = parser.parse(REPO_DIR, CODE_DIR);

  let defs: string[] = [];
  for (let n of nodes) {
    const name = path.basename(n.name);
    defs.push(`${name}(${n.startLine}:${n.endLine})`);
  }

  t.deepEqual(defs, ['d.py:D1(1:3)', 'd.py:D2(6:8)', 'd.py:D(11:12)'], 'unexpected definitions');
})
