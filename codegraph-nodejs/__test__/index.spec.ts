import test from 'ava';

import * as path from 'path';
const REPO_DIR = path.join(path.dirname(path.dirname(import.meta.dirname)), 'examples', 'go', 'demo');
const DB_DIR = path.join(REPO_DIR, "kuzu_db");

import * as codegraph from '../index.js';

const config = {
  ignorePatterns: [
    "*",
    "!main.go",
    "!types.go",
  ],
};
const graph = new codegraph.CodeGraph(DB_DIR, REPO_DIR, config);

test('parsing nodes', (t) => {
  graph.index(REPO_DIR, false);

  const MAIN_GO = path.join(REPO_DIR, "main.go");
  const snippets = graph.getFuncParamTypes(MAIN_GO, 37);

  let types: string[] = [];
  for (let s of snippets) {
    types.push(`--> ${s.path}:${s.startLine}:${s.endLine}`);
  }
  types.sort();
  t.deepEqual(types, ['types.go:3:6', 'types.go:8:11'], 'unexpected types');

  graph.clean(true);
})