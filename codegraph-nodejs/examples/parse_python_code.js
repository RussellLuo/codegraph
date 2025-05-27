import * as codegraph from '../index.js';

const graph = new CodeGraph("./graph/test_db");

graph.index("/your/repo/path", "/your/code/path");

const nodes = graph.query("MATCH (n) RETURN *");
for (let n of nodes ) {
  console.log(`${n.name}:${n.startLine}:${n.endLine}`);
  console.log(n.code);
}

graph.clean(true);
