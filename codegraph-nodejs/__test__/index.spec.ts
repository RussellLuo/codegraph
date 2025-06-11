import test from 'ava';

import * as path from 'path';
const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'python');
const DB_DIR = path.join(REPO_DIR, 'db');

import * as codegraph from '../index.js';

const config: codegraph.Config = {
  // Only include "d.py"
  ignorePatterns: [
    "*",
    "!d.py",
  ],
};
const graph = new codegraph.CodeGraph(DB_DIR, REPO_DIR, config);

test('parsing nodes', (t) => {
  graph.index([REPO_DIR]);
  graph.clean(true);
})