import argparse
from collections import defaultdict, OrderedDict
import dataclasses
import fnmatch
import os

import tree_sitter
import tree_sitter_python

from database import (
    Database,
    NodeType,
    EdgeType,
    Point,
    Node,
    Relationship,
)

PY_LANGUAGE = tree_sitter.Language(tree_sitter_python.language())
parser = tree_sitter.Parser(PY_LANGUAGE)

with open("./references.scm", "r") as f:
    QUERY = PY_LANGUAGE.query(f.read())


FILE_CONTAINING_NODE_CACHE = {}


@dataclasses.dataclass
class Import:
    node_name: str
    import_: str
    alias: str = ""


@dataclasses.dataclass
class Inherit:
    class_node: Node
    superclass_name: str


class Parser:
    def __init__(self, db: Database, repo_path: str, module_search_paths: list[str]):
        self.db: Database = db
        self.repo_path: str = repo_path
        self.module_search_paths: list[str] = module_search_paths

        self.nodes: OrderedDict[str, Node] = OrderedDict()
        self.relationships: list[Relation] = []
        self.file_imports: dict[Node, list[Import]] = defaultdict(list)
        self.file_inherits: dict[Node, list[Inherit]] = defaultdict(list)

    def _add_node(self, node: Node) -> None:
        self.nodes[node.name] = node

    def parse_code(self, file_node: Node):
        """解析源代码并提取模块级元素"""
        tree = parser.parse(file_node.code.encode(), encoding="utf8")

        # 遍历根节点的所有子节点
        for child in tree.root_node.children:
            # 处理import语句
            match child.type:
                case "import_statement":
                    imps = self.parse_import(file_node, child)
                    self.file_imports[file_node].extend(imps)

                case "import_from_statement":
                    imps = self.parse_from_import(file_node, child)
                    self.file_imports[file_node].extend(imps)

                # 提取模块级变量定义
                case "expression_statement":
                    node = self.parse_variable(file_node, child)
                    if node:
                        self._add_node(node)
                        self.relationships.append(
                            Relationship(
                                type=EdgeType.CONTAINS,
                                from_=file_node,
                                to_=node,
                            )
                        )

                # 提取函数定义
                case "function_definition":
                    node = self.parse_function(file_node, child)
                    self._add_node(node)
                    self.relationships.append(
                        Relationship(
                            type=EdgeType.CONTAINS,
                            from_=file_node,
                            to_=node,
                        )
                    )

                # 提取类定义
                case "class_definition":
                    node = self.parse_class(file_node, child)
                    self._add_node(node)
                    self.relationships.append(
                        Relationship(
                            type=EdgeType.CONTAINS,
                            from_=file_node,
                            to_=node,
                        )
                    )

                    superclasses_node = child.child_by_field_name("superclasses")
                    if superclasses_node:
                        for item in superclasses_node.children:
                            # class A(Base) or class A(mod.Base)
                            if item.type in ("identifier", "attribute"):
                                superclass_name = item.text.decode().strip()
                                self.file_inherits[file_node].append(
                                    Inherit(
                                        class_node=node,
                                        superclass_name=superclass_name,
                                    )
                                )

                    cls_body_node = child.child_by_field_name("body")
                    for body_child in cls_body_node.children:
                        match body_child.type:
                            case "function_definition":
                                meth_node = self.parse_method(node, body_child)
                                self._add_node(meth_node)
                                self.relationships.append(
                                    Relationship(
                                        type=EdgeType.CONTAINS,
                                        from_=node,
                                        to_=meth_node,
                                    )
                                )

    def parse_import(self, file_node: Node, child: tree_sitter.Node) -> list[Node]:
        # 处理普通import语句(如import os, sys)
        imps: list[Node] = []

        for import_item in child.children[1:]:
            if import_item.type == ",":
                continue
            module_name = import_item.text.decode().strip()
            alias = ""
            if " as " in module_name:
                module_name, alias = module_name.split(" as ")
            imps.append(
                Import(
                    node_name=self.resolve_module_name(module_name),
                    import_=module_name,
                    alias=alias,
                )
            )

        return imps

    def parse_from_import(self, file_node: Node, child: tree_sitter.Node) -> list[Node]:
        # 处理from...import语句(如from collections import defaultdict)
        imps: list[Node] = []

        module_path = child.children[1].text.decode().strip()
        for import_item in child.children[3:]:
            if import_item.type == ",":
                continue
            import_name = import_item.text.decode().strip()
            module_name = f"{module_path}.{import_name}"
            alias = ""
            if " as " in module_name:
                module_name, alias = module_name.split(" as ")
            module_name = self.relative_to_absolute(file_node.name, module_name)
            imps.append(
                Import(
                    node_name=self.resolve_module_name(module_name),
                    import_=import_name,
                    alias=alias,
                )
            )

        return imps

    def parse_variable(self, file_node: Node, child: tree_sitter.Node) -> Node | None:
        assignment = child.children[0]
        if assignment.type == "augmented_assignment":
            # Skip for now
            return None
        identifier = assignment.child_by_field_name("left")
        if not identifier:
            return None

        var_name = identifier.text.decode()
        code = assignment.text.decode()
        return Node(
            type=NodeType.VARIABLE,
            name=f"{file_node.name}:{var_name}",
            code=code,
            start=self._create_point(identifier.start_point),
            end=self._create_point(identifier.end_point),
        )

    def parse_function(self, file_node: Node, child: tree_sitter.Node) -> Node:
        name_node = child.child_by_field_name("name")
        func_name = name_node.text.decode().strip()
        code = child.text.decode()
        return Node(
            type=NodeType.FUNCTION,
            name=f"{file_node.name}:{func_name}",
            code=code,
            start=self._create_point(child.start_point),
            end=self._create_point(child.end_point),
        )

    def parse_class(self, file_node: Node, child: tree_sitter.Node) -> Node:
        name_node = child.child_by_field_name("name")
        class_name = name_node.text.decode().strip()
        code = child.text.decode()
        return Node(
            type=NodeType.CLASS,
            name=f"{file_node.name}:{class_name}",
            code=code,
            start=self._create_point(child.start_point),
            end=self._create_point(child.end_point),
        )

    def parse_method(self, class_node: Node, child: tree_sitter.Node) -> Node:
        name_node = child.child_by_field_name("name")
        func_name = name_node.text.decode().strip()
        code = child.text.decode()
        return Node(
            type=NodeType.FUNCTION,
            name=f"{class_node.name}.{func_name}",
            code=code,
            start=self._create_point(child.start_point),
            end=self._create_point(child.end_point),
        )

    def _create_point(self, ts_point: tree_sitter.Point) -> Point:
        return Point(
            line=ts_point[0] + 1,  # 0-based to 1-based line number
            column=ts_point[1] + 1,  # 0-based to 1-based column number
        )

    def relative_to_absolute(self, current_file_path: str, module_name: str) -> str:
        """
        将相对模块名转换为绝对模块名

        参数:
            module_name: 相对模块名(如".module"或"..parent.module")

        返回:
            绝对模块名(如"package.module"或"parent.module")
        """
        # 分割模块名为部分
        parts = module_name.split(".")

        # 计算相对导入的层级(开头的点数)
        level = 0
        while level < len(parts) and parts[level] == "":
            level += 1

        if level == 0:
            # absolute import
            return module_name  # 不是相对导入

        # 计算基础路径(向上level-1级)
        base_path = os.path.dirname(current_file_path)
        for _ in range(level - 1):
            base_path = os.path.dirname(base_path)

        # 获取模块剩余部分
        module_parts = parts[level:]
        # 构建绝对路径
        absolute_path = os.path.join(base_path, *module_parts)

        # 转换为模块格式(替换路径分隔符为点)
        return absolute_path.replace(os.sep, ".")

    def _get_module_file_path(self, module_name: str) -> tuple[str, bool]:
        """
        根据模块名查找对应的Python文件路径

        参数:
            module_name: 要查找的模块名(点分隔格式，如"package.module")

        返回:
            str: 模块文件的相对路径(相对于repo_path)，如果未找到则返回空字符串
            bool: 是否位于标准库或三方库
        """

        base_paths = [self.repo_path] + self.module_search_paths
        candidates = [
            module_name.replace(".", os.sep) + ".py",
            os.path.join(module_name.replace(".", os.sep), "__init__.py"),
        ]
        for base_path in base_paths:
            for candidate in candidates:
                path = os.path.join(base_path, candidate)
                if os.path.isfile(path):
                    in_repo = path.startswith(self.repo_path)
                    if in_repo:
                        return os.path.relpath(path, self.repo_path), False
                    else:
                        # in std lib or 3rd lib, return absolute path
                        return path, True

        return "", False

    def parse_lib_node(self, node_name: str) -> None:
        def _parse() -> Node:
            file_path, attribute_name = node_name, ""
            if ":" in file_path:
                file_path, attribute_name = file_path.rsplit(":", 1)

            file_node = self._parse_file(file_path)
            if not attribute_name:
                file_node.code = ""  # Do not store the code of stdlib or 3rd-lib
                return file_node

            # function or class
            tree = parser.parse(file_node.code.encode(), encoding="utf8")

            # 遍历根节点的所有子节点
            for child in tree.root_node.children:
                # 处理import语句
                match child.type:
                    # 提取函数定义
                    case "function_definition":
                        node = self.parse_function(file_node, child)
                        if node.name == attribute_name:
                            return node

                    # 提取类定义
                    case "class_definition":
                        node = self.parse_class(file_node, child)
                        if node.name == attribute_name:
                            return node

            # cases node is imported from another module
            # set the type to unknown for now
            return Node(type=NodeType.UNPARSED, name=node_name)

        node = _parse()
        self.db.upsert_node(node)

    def resolve_module_name(self, module_name: str) -> str:
        """
        Resolve the module name to the actual name of the destination node (file or class or function).

        参数:
            module_name: 要解析的模块名(可以是模块路径或模块属性路径，如"module"或"module.attribute")

        返回:
            解析后的节点名称，如果解析失败则返回None
        """
        # 尝试解析整个路径作为模块
        file_path, in_lib = self._get_module_file_path(module_name)
        if file_path:
            if in_lib:
                self.parse_lib_node(file_path)
            return file_path

        # 尝试解析模块属性(如module.attribute)
        if "." in module_name:
            module_part, attribute_part = module_name.rsplit(".", 1)
            module_path, in_lib = self._get_module_file_path(module_part)
            if module_path:
                node_name = f"{module_path}:{attribute_part}"
                if in_lib:
                    self.parse_lib_node(node_name)
                return node_name

        # 未识别的模块，暂时无法解析具体文件路径，原样返回
        return module_name

    def parse_file(self, file_path: str, base_path: str = "") -> Node:
        """处理单个文件并返回解析结果"""
        file_node = self._parse_file(file_path, base_path)
        self.parse_code(file_node)
        return file_node

    def _parse_file(self, file_path: str, base_path: str = "") -> Node:
        with open(file_path, "rb") as file:
            source_code = file.read()

        rel_file_path = file_path
        if base_path:
            rel_file_path = os.path.relpath(file_path, base_path)

        return Node(name=rel_file_path, type=NodeType.FILE, code=source_code.decode())

    @staticmethod
    def filter_path(path: str) -> bool:
        """参考.gitignore过滤路径"""
        ignore_patterns = [
            # 默认忽略的目录
            "__pycache__",
            "tests",
            "test",
            "venv",
            "env",
            # 从.gitignore提取的规则
            ".DS_Store",
            "results/",
            "*.py[cod]",
            "*$py.class",
            "*.so",
            "build/",
            "develop-eggs/",
            "dist/",
            "downloads/",
            "eggs/",
            ".eggs/",
            "lib/",
            "lib64/",
            "parts/",
            "sdist/",
            "var/",
            "wheels/",
            "pip-wheel-metadata/",
            "share/python-wheels/",
            "*.egg-info/",
            "*.egg",
            "htmlcov/",
            ".tox/",
            ".nox/",
            ".coverage",
            ".coverage.*",
            ".cache",
            ".hypothesis/",
            ".pytest_cache/",
            "instance/",
            ".webassets-cache/",
            ".scrapy/",
            "docs/_build/",
            "target/",
            ".ipynb_checkpoints/",
            "profile_default/",
            ".python-version",
            "__pypackages__/",
            ".env",
            ".venv/",
            "env/",
            "venv/",
            "ENV/",
            "env.bak/",
            "venv.bak/",
            ".spyderproject",
            ".spyproject",
            ".ropeproject/",
            "site/",
            ".mypy_cache/",
            ".pyre/",
            "*.xml",
            "*.gif",
        ]

        # 检查路径是否匹配任何忽略模式
        path_parts = path.split(os.sep)
        for part in path_parts:
            for pattern in ignore_patterns:
                # 处理目录模式(以/结尾)
                if pattern.endswith("/"):
                    if fnmatch.fnmatch(part, pattern[:-1]):
                        return False
                else:
                    if fnmatch.fnmatch(part, pattern):
                        return False
        return True

    def parse_path(self, path: str, root: str = ""):
        """处理指定路径(文件或目录)并返回解析结果"""
        if not self.filter_path(path):
            return

        if os.path.isfile(path):
            file_node = self.parse_file(path, root)
            self._add_node(file_node)
            return

        # 处理目录
        dir_node = self.create_directory_node(path, root)
        self._add_node(dir_node)
        parent_nodes = {path: dir_node}  # 缓存父目录节点

        for base, dirs, files in os.walk(path):
            # 先过滤不需要的目录
            dirs[:] = [d for d in dirs if self.filter_path(os.path.join(base, d))]

            # 获取当前目录节点(已缓存)
            current_dir_node = parent_nodes[base]

            # 处理文件
            for file in files:
                file_path = os.path.join(base, file)
                if not file_path.endswith(".py"):
                    continue

                file_node = self.parse_file(file_path, root)
                self._add_node(file_node)
                self.relationships.append(
                    Relationship(
                        type=EdgeType.CONTAINS,
                        from_=current_dir_node,
                        to_=file_node,
                    )
                )

            # 处理子目录并建立关系
            for dir in dirs:
                dir_path = os.path.join(base, dir)
                child_dir_node = self.create_directory_node(dir_path, root)
                self._add_node(child_dir_node)
                self.relationships.append(
                    Relationship(
                        type=EdgeType.CONTAINS,
                        from_=current_dir_node,
                        to_=child_dir_node,
                    )
                )
                # 缓存子目录节点供后续使用
                parent_nodes[dir_path] = child_dir_node

    def resolve_superclass_name(self, inherit: Inherit, file_node: Node) -> str:
        """
        Resolve the superclass name to the actual name of the superclass node.
        """
        name = inherit.superclass_name
        if "." not in name:
            # cases where superclass is defined inside the file (module) itself
            node_name = f"{file_node.name}:{name}"
            if self.db.has_node(node_name):
                return node_name

            # cases where superclass might be imported from another module
            result = self.db.execute(
                """
                MATCH (a)-[b:IMPORTS]-(c)
                WHERE a.name = $name AND c.type IN ['class', 'unparsed']
                RETURN c.name, b.alias;
                """,
                parameters={"name": file_node.name},
            )
            if not result:
                return ""

            for r in result:
                imp_node_name, imp_node_alias = r
                imp_class_name = imp_node_alias or imp_node_name.split(":")[-1]
                if imp_class_name == name:
                    return imp_node_name

            return ""

        # cases where superclass might be from an imported module
        result = self.db.execute(
            """
            MATCH (a)-[b:IMPORTS]-(c)
            WHERE a.name = $name AND c.type = 'file'
            RETURN c.name, b.import, b.alias;
            """,
            parameters={"name": file_node.name},
        )
        if not result:
            return ""

        mod_name, superclass_name = name.rsplit(".", 1)
        for imp_node_name, imp_node_import, imp_node_alias in result:
            if mod_name == (imp_node_alias or imp_node_import):
                return f"{imp_node_name}:{superclass_name}"

        return ""

    def _get_containing_node(self, node: Node) -> list[Node]:
        containing_nodes = FILE_CONTAINING_NODE_CACHE.get(node.name)
        if not containing_nodes:
            containing_nodes = self.db.traverse_nodes(
                node.name,
                "downstream",
                relationship_type_filter=[EdgeType.IMPORTS, EdgeType.CONTAINS],
            )
            FILE_CONTAINING_NODE_CACHE[node.name] = containing_nodes
        return containing_nodes

    def resolve_reference_relationships(self, node: Node) -> list[Relationship]:
        referenced_nodes: set[Node] = set()

        def parse_reference_names(code: str) -> list[str]:
            tree = parser.parse(code.encode(), encoding="utf8")
            captures = QUERY.captures(tree.root_node)
            if not captures:
                return []
            return [n.text.decode() for n in captures["name.reference"]]

        def resolve(node: Node, reference_names: list[str]):
            result = self.db.traverse_nodes(
                node.name,
                "upstream",
                node_type_filter=[NodeType.FILE],
                relationship_type_filter=[EdgeType.CONTAINS],
                limit=1,
            )
            if not result:
                return
            parent_file_node = result[0][0]

            containing_nodes = self._get_containing_node(parent_file_node)
            for n, r in containing_nodes:
                for name in reference_names:
                    match r.type:
                        case EdgeType.IMPORTS:
                            imp_name = r.alias or r.import_
                            if name == imp_name:
                                # Reference module-level class/function/variable imported from a file (module)
                                referenced_nodes.add(n)
                            elif name.startswith(imp_name):
                                # Reference module-level class/function/variable from an imported file (module)
                                attr_name = name[
                                    len(imp_name) + 1 :
                                ]  # Remove one more "."
                                if n.type == NodeType.FILE:
                                    # attribute from file
                                    attr_node_name = f"{n.name}:{attr_name}"
                                elif n.type in (NodeType.CLASS, NodeType.UNPARSED):
                                    # methods from class
                                    attr_node_name = f"{n.name}.{attr_name}"
                                elif n.type == NodeType.VARIABLE:
                                    # the varialbe might be an instance of a class?
                                    # TODO: not supported fo now.
                                    attr_node_name = ""
                                else:  # NodeType.FUNCTION
                                    # TODO: more complicated, not supported for now.
                                    attr_node_name = ""

                                attr_node = self.db.get_node(attr_node_name)
                                if not attr_node and attr_node_name:
                                    # might be an attribute from external lib
                                    # let it be unparsed for now.
                                    attr_node = Node(
                                        type=NodeType.UNPARSED,
                                        name=attr_node_name,
                                    )
                                    self.db.upsert_node(attr_node)
                                if attr_node:
                                    referenced_nodes.add(attr_node)

                        case EdgeType.CONTAINS:
                            # Reference module-level class or function or variable defined in the same file (module)
                            n_short_name = n.short_names[0]
                            if name == n_short_name:
                                referenced_nodes.add(n)
                            elif name.startswith(n_short_name):
                                attr_name = name[
                                    len(n_short_name) + 1 :
                                ]  # Remove one more "."
                                if n.type == NodeType.CLASS:
                                    # methods from class
                                    attr_node_name = f"{n.name}.{attr_name}"
                                elif n.type == NodeType.VARIABLE:
                                    # the varialbe might be an instance of a class?
                                    # TODO: not supported fo now.
                                    attr_node_name = ""
                                else:  # NodeType.FUNCTION
                                    # TODO: more complicated, not supported for now.
                                    attr_node_name = ""

                                attr_node = self.db.get_node(attr_node_name)
                                if not attr_node and attr_node_name:
                                    # might be an attribute (inhertis) from external lib
                                    # let it be unparsed for now.
                                    attr_node = Node(
                                        type=NodeType.UNPARSED,
                                        name=attr_node_name,
                                    )
                                    self.db.upsert_node(attr_node)
                                if attr_node:
                                    referenced_nodes.add(attr_node)

        match node.type:
            case NodeType.CLASS:
                names = parse_reference_names(node.code)
                if names:
                    resolve(node, names)

            case NodeType.FUNCTION:
                names = parse_reference_names(node.code)
                if names:
                    resolve(node, names)

            case NodeType.VARIABLE:
                names = parse_reference_names(node.code)
                if names:
                    resolve(node, names)

        return [
            Relationship(
                type=EdgeType.REFERENCES,
                from_=node,
                to_=n,
            )
            for n in referenced_nodes
        ]

    def create_directory_node(self, path: str, root: str = "") -> Node:
        """添加目录节点到结果列表"""
        rel_path = os.path.relpath(path, root) if root else path
        return Node(name=rel_path, type=NodeType.DIRECTORY)

    def save_to_db(self, output: str) -> None:
        # Save nodes
        self.db.batch_add_nodes(*self.nodes.values())

        # Save CONTAINS relationships
        self.db.batch_add_relationships(*self.relationships)

        # Save IMPORTS relationships
        imports_relationships: list[Relationship] = []
        for file_node, imps in self.file_imports.items():
            for imp in imps:
                imp_node = self.db.get_node(imp.node_name)
                if not imp_node:
                    continue
                imports_relationships.append(
                    Relationship(
                        type=EdgeType.IMPORTS,
                        from_=file_node,
                        to_=imp_node,
                        import_=imp.import_,
                        alias=imp.alias,
                    )
                )
        self.db.batch_add_relationships(*imports_relationships)

        # Save INHERITS relationships
        inherits_relationships: list[Relationship] = []
        for file_node, inherits in self.file_inherits.items():
            for inherit in inherits:
                superclass_node_name = self.resolve_superclass_name(inherit, file_node)
                if not superclass_node_name:
                    continue

                superclass_node = self.db.get_node(superclass_node_name)
                if not superclass_node:
                    # The superclass is from an imported external library, create an UNPARSED node.
                    superclass_node = Node(
                        name=superclass_node_name,
                        type=NodeType.UNPARSED,
                    )
                    self.db.upsert_node(superclass_node)

                inherits_relationships.append(
                    Relationship(
                        type=EdgeType.INHERITS,
                        from_=inherit.class_node,
                        to_=superclass_node,
                    )
                )
        self.db.batch_add_relationships(*inherits_relationships)

        # Save REFERENCES relationships
        references_relationships: list[Relationship] = []
        for _, node in self.nodes.items():
            relationships = self.resolve_reference_relationships(node)
            # Filter out REFERENCES relationship to FILE node
            # FIXME: there is a bug.
            # example:
            #   from pylint.lint.run import Run
            # will be resolved to a FILE node:
            #   /opt/miniconda3/envs/crmaestro/lib/python3.12/site-packages/pylint/lint/Run.py
            # while the actual node is:
            #   /opt/miniconda3/envs/crmaestro/lib/python3.12/site-packages/pylint/lint/run.py:Run
            relationships = [r for r in relationships if r.to_.type != NodeType.FILE]
            if relationships:
                references_relationships.extend(relationships)
        self.db.batch_add_relationships(*references_relationships)


def main():
    """主函数"""
    parser = argparse.ArgumentParser(description="Python源代码解析工具")
    parser.add_argument("repo", default="", help="项目根路径")
    parser.add_argument("--include", help="要解析的Python文件或目录路径")
    parser.add_argument("--mod_search_path", default="", help="模块搜索路径")
    parser.add_argument("--output", default="./output", help="输出目录")
    args = parser.parse_args()

    include = args.include or args.repo

    module_search_paths = (
        args.mod_search_path.split(":") if args.mod_search_path else []
    )
    db = Database("./graph/db", tmp_data="./graph/data")
    db.delete_all()

    ast_parser = Parser(db, args.repo, module_search_paths)
    ast_parser.parse_path(include, args.repo)
    ast_parser.save_to_db(args.output)


if __name__ == "__main__":
    main()
