import * as codegraph from '../index.js';
import * as path from 'path';

const REPO_DIR = "/Users/russellluo/Projects/work/opencsg/projects/starhub-server";
const DB_DIR = path.join(REPO_DIR, "kuzu_db");

const config = {
  ignorePatterns: [
    "*",
    "!*.go",
  ],
};

codegraph.initLogger(codegraph.LogLevel.Info);

const graph = new codegraph.CodeGraph(DB_DIR, REPO_DIR, config);
graph.clean(true);
graph.index(REPO_DIR, false);

const snippets = graph.getFuncParamTypes(
  path.join(REPO_DIR, "builder/store/database/mirror.go"),
  355
);
for (let s of snippets) {
  console.log(`--> ${s.path}:${s.startLine}:${s.endLine}\n${s.content}`);
}

//graph.clean(true);
