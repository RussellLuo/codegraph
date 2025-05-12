from collections import defaultdict
import json

from mcp.server.fastmcp import FastMCP
import kuzu

from database import Database, Node

mcp = FastMCP("Demo")
db = Database("./graph/db")


@mcp.tool()
def search_entities(search_terms: list[str]) -> str:
    """Searches the codebase to retrieve relevant code entities based on given query terms.

    Note:
    1. If `search_terms` are provided, it searches for code snippets based on each term:
        - If a term is formatted as 'file_path:QualifiedName' (e.g., 'src/helpers/math_helpers.py:MathUtils.calculate_sum') ,
          or just 'file_path', the corresponding complete code is retrieved or file content is retrieved.
        - If a term matches a file, class, or function name, matched entities are retrieved.

    Args:
        search_terms (Optional[List[str]]): A list of names, keywords, or code snippets to search for within the codebase.
            Terms can be formatted as 'file_path:QualifiedName' to search for a specific module or entity within a file
            (e.g., 'src/helpers/math_helpers.py:MathUtils.calculate_sum') or as 'file_path' to retrieve the complete content
            of a file. This can also include potential function names, class names, or general code fragments.

    Returns:
        str: The search results, which may include code snippets, matching entities, or complete file content.


    Example Usage:
        # Search for the full content of a specific file
        result = search_entities(search_terms=['src/my_file.py'])

        # Search for a specific class
        result = search_entities(search_terms=['src/my_file.py:MyClass'])

        # Search for a keyword in the style of an unqualified name
        result = search_entities(search_terms=["MyClass"]
    """
    nodes: list[Node] = []
    for term in search_terms:
        term = term.strip()
        term_node = db.get_node(term)
        if term_node:
            nodes.append(term_node)

        result = db.execute(
            """
            MATCH (a)
            WHERE $term IN a.short_names
            RETURN a;
            """,
            parameters={"term": term},
        )
        for r in result:
            node = Node.from_dict(r[0])
            nodes.append(node)

    # Return the result as a JSON string
    return json.dumps(
        [
            {
                "name": node.name,
                "type": node.type,
            }
            for node in nodes
        ],
        indent=2,
    )


@mcp.tool()
def traverse_graph(
    start_entities: list[str],
    direction: str = "downstream",
    traversal_depth: int = 1,
    entity_type_filter: list[str] | None = None,
    dependency_type_filter: list[str] | None = None,
) -> str:
    """Analyzes and displays the dependency structure around specified entities in a code graph.

    This function searches and presents relationships and dependencies for the specified entities (such as classes, functions, files, or directories) in a code graph.
    It explores how the input entities relate to others, using defined types of dependencies, including 'contains', 'imports', 'invokes' and 'inherits'.
    The search can be controlled to traverse upstream (exploring dependencies that entities rely on) or downstream (exploring how entities impact others), with optional limits on traversal depth and filters for entity and dependency types.

    Example Usage:
    1. Exploring Outward Dependencies:
        ```
        traverse_graph(
            start_entities=['src/module_a.py:ClassA'],
            direction='downstream',
            traversal_depth=2,
            entity_type_filter=['class', 'function'],
            dependency_type_filter=['invokes', 'imports']
        )
        ```
        This retrieves the dependencies of `ClassA` up to 2 levels deep, focusing only on classes and functions with 'invokes' and 'imports' relationships.

    2. Exploring Inward Dependencies:
        ```
        traverse_graph(
            start_entities=['src/module_b.py:FunctionY'],
            direction='upstream',
            traversal_depth=-1
        )
        ```
        This finds all entities that depend on `FunctionY` without restricting the traversal depth.

    Notes:
    * Traversal Control: The `traversal_depth` parameter specifies how deep the function should explore the graph starting from the input entities.
    * Filtering: Use `entity_type_filter` and `dependency_type_filter` to narrow down the scope of the search, focusing on specific entity types and relationships.
    * Graph Context: The function operates on a pre-built code graph containing entities (e.g., files, classes and functions) and dependencies representing their interactions and relationships.

    Parameters:
    ----------
    start_entities : list[str]
        List of entities (e.g., class, function, file, or directory paths) to begin the search from.
        - Entities representing classes or functions must be formatted as "file_path:QualifiedName"
          (e.g., `interface/C.py:C.method_a.inner_func`).
        - For files or directories, provide only the file or directory path (e.g., `src/module_a.py` or `src/`).

    direction : str, optional
        Direction of traversal in the code graph; allowed options are:
        - 'downstream': Traversal to explore dependencies that the specified entities rely on (how they depend on others).
        - 'upstream': Traversal to explore the effects or interactions of the specified entities on others
          (how others depend on them).
        Default is 'downstream'.

    traversal_depth : int, optional
        Maximum depth of traversal. A value of -1 indicates unlimited depth (subject to a maximum limit).
        Must be either `-1` or a non-negative integer (≥ 0).
        Default is 1.

    entity_type_filter : list[str], optional
        List of entity types (e.g., 'class', 'function', 'file', 'directory') to include in the traversal.
        If None, all entity types are included.
        Default is None.

    dependency_type_filter : list[str], optional
        List of dependency types (e.g., 'contains', 'imports', 'invokes', 'inherits') to include in the traversal.
        If None, all dependency types are included.
        Default is None.

    Returns:
    -------
    result : object
        An object representing the traversal results, which includes discovered entities and their dependencies.
    """
    rtns = {
        node: traverse_json_structure(
            node,
            direction,
            traversal_depth,
            entity_type_filter,
            dependency_type_filter,
        )
        for node in start_entities
    }
    rtn_str = json.dumps(rtns)
    return rtn_str.strip()


def traverse_json_structure(
    start_node: str,
    direction: str,
    depth: int = 1,
    entity_type_filter: list[str] | None = None,
    dependency_type_filter: list[str] | None = None,
) -> dict:
    result = db.execute(
        """
        MATCH (a)
        WHERE a.name = $start_node
        RETURN a.type;
        """,
        parameters={"start_node": start_node},
    )
    if not result:
        return {}

    node_table = result[0][0].title()

    depth = min(
        depth if depth > 0 else 1, 5
    )  # Limit depth to 5 for performance reasons.
    level = f"*1..{depth}"

    rel_labels = ""
    if dependency_type_filter:
        rel_labels = "|".join(f":{dep.upper()}" for dep in dependency_type_filter)
    relationship = f"-[b{rel_labels}{level}]-"

    match direction:
        case "downstream":
            relationship = f"{relationship}>"
        case "upstream":
            relationship = f"<{relationship}"
        case _:  # Including "both"
            pass

    target_nodes = ""
    if entity_type_filter:
        target_nodes = f":{':'.join(entity_type_filter).title()}"

    result = db.execute(
        f"""
        MATCH (a:{node_table}){relationship}(c{target_nodes})
        WHERE a.name = $start_node
        RETURN c.type, c.name;
        """,
        parameters={"start_node": start_node},
    )
    x = defaultdict(list)
    for r in result:
        x[r[0]].append(r[1])

    return x
