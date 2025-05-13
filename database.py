from __future__ import annotations

from collections import defaultdict
import dataclasses
import enum
import json
import os
import shutil

import kuzu


class NodeType(str, enum.Enum):
    UNPARSED = "unparsed"
    DIRECTORY = "directory"
    FILE = "file"
    VARIABLE = "variable"
    FUNCTION = "function"
    CLASS = "class"


class EdgeType(str, enum.Enum):
    CONTAINS = "contains"
    IMPORTS = "imports"
    INVOKES = "invokes"
    INHERITS = "inherits"


@dataclasses.dataclass
class Point:
    line: int
    column: int


@dataclasses.dataclass
class Node:
    type: NodeType
    name: str
    code: str = ""
    start: Point | None = None
    end: Point | None = None

    @classmethod
    def from_dict(cls, data: dict) -> Node:
        return Node(
            type=NodeType(data["type"]),
            name=data["name"],
            code=data.get("code", ""),
            start=Point(line=data.get("start_line", 0), column=0),  # no column info
            end=Point(line=data.get("end_line", 0), column=0),  # no column info
        )

    def __hash__(self):
        return hash((self.type, self.name))

    def __eq__(self, other: None):
        return self.type == other.type and self.name == other.name

    @property
    def short_names(self) -> list[str]:
        def make_names(name: str) -> list[str]:
            lower = name.lower()
            if lower != name:
                return [name, lower]
            return [name]

        if ":" not in self.name:
            # "src/a.py" => a
            file_name = self.name.rsplit("/", 1)[-1]
            return make_names(file_name)

        # "src/a.py:A" => A, a
        attr_name = self.name.rsplit(":", 1)[-1]
        if "." not in attr_name:
            return make_names(attr_name)

        # "src/a.py:A.meth" => meth
        sub_attr_name = attr_name.rsplit(".", 1)[-1]
        return make_names(sub_attr_name)

    def to_dict(self) -> dict:
        match self.type:
            case NodeType.UNPARSED | NodeType.DIRECTORY:
                return {
                    "name": self.name,
                    "type": self.type.value,
                    "short_names": self.short_names,
                }
            case NodeType.FILE:
                return {
                    "name": self.name,
                    "type": self.type.value,
                    "short_names": self.short_names,
                    "code": self.code,
                }
            case NodeType.VARIABLE:
                pass
            case NodeType.FUNCTION | NodeType.CLASS:
                return {
                    "name": self.name,
                    "type": self.type.value,
                    "short_names": self.short_names,
                    "code": self.code,
                    "start_line": self.start.line,
                    "end_line": self.end.line,
                }
            case _:
                raise ValueError(f"Unsupport node type: {self.type}")


@dataclasses.dataclass
class Relationship:
    type: EdgeType
    from_: Node
    to_: Node
    import_: str | None = None
    alias: str | None = None

    def to_dict(self) -> dict:
        match self.type:
            case EdgeType.CONTAINS | EdgeType.INHERITS:
                return {
                    "from": self.from_.name,
                    "to": self.to_.name,
                    "type": self.from_to,
                }
            case EdgeType.IMPORTS:
                return {
                    "from": self.from_.name,
                    "to": self.to_.name,
                    "type": self.from_to,
                    "import": self.import_,
                    "alias": self.alias,
                }
            case _:
                raise ValueError(f"Unsupport edge type: {self.type}")

    @property
    def from_to(self) -> str:
        return f"{self.from_.type.value}_{self.to_.type.value}"


class Database:
    def __init__(self, db_path: str, tmp_data: str = "./tmp_data"):
        db: kuzu.Database = kuzu.Database(db_path)
        self.conn: kuzu.Connection = kuzu.Connection(db)
        self.tmp_data = tmp_data

    def execute(self, query: str, parameters: dict | None = None) -> list:
        result: list = []
        response = self.conn.execute(query, parameters=parameters)
        while response.has_next():
            result.append(response.get_next())
        return result

    def upsert_node(self, node: Node) -> None:
        table_name = node.type.title()
        data = "{" + ", ".join(f"{k}: {v!r}" for k, v in node.to_dict().items()) + "}"
        self.execute(f"MERGE (n:{table_name} {data}) RETURN n.*;")

    def batch_add_nodes(self, *nodes: Node) -> None:
        self.execute("Load json;")
        group_by_type = defaultdict(list)
        for n in nodes:
            group_by_type[n.type].append(n.to_dict())

        for typ, data in group_by_type.items():
            file_path = os.path.join(self.tmp_data, typ + ".json")
            with open(file_path, "w+", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
            self.execute(f"COPY {typ.title()} FROM '{file_path}';")

    def batch_add_relationships(self, *relationships: Relationship) -> None:
        self.execute("Load json;")
        group_by_type = defaultdict(lambda: defaultdict(list))
        for r in relationships:
            group_by_type[r.type][r.from_to].append(r.to_dict())

        for typ, data in group_by_type.items():
            for from_to, d in data.items():
                table_name = typ.upper()
                from_type, to_type = from_to.split("_")
                filename = f"{table_name}_{from_to}.json"
                file_path = os.path.join(self.tmp_data, filename)
                with open(file_path, "w+", encoding="utf-8") as f:
                    json.dump(d, f, indent=2, ensure_ascii=False)
                self.execute(
                    f"COPY {table_name} FROM '{file_path}' (from='{from_type.title()}', to='{to_type.title()}');"
                )

    def delete_all(self):
        # Delete all records
        self.execute("MATCH (n) DETACH DELETE n;")
        if self.tmp_data:
            if os.path.exists(self.tmp_data):
                shutil.rmtree(self.tmp_data)
            os.makedirs(self.tmp_data, exist_ok=True)

    def get_node(self, name: str) -> Node | None:
        result = self.execute(
            """
            MATCH (a)
            WHERE a.name = $name
            RETURN a;
            """,
            parameters={"name": name},
        )
        if not result:
            return None

        data = result[0][0]
        return Node.from_dict(data)

    def has_node(self, name: str) -> bool:
        result = self.execute(
            """
            MATCH (a)
            WHERE a.name = $name
            RETURN a;
            """,
            parameters={"name": name},
        )
        return bool(result)
