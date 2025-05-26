from collections import defaultdict
import json

from mcp.server.fastmcp import FastMCP
import kuzu

from database import Database, Node, NodeType

mcp = FastMCP("Demo")
db = Database("./graph/db")


@mcp.tool()
def locate_entities(search_terms: list[str]) -> str:
    """Searches the codebase to retrieve the locations of relevant entities (file/class/function) based on given query terms.

    Args:
        search_terms (List[str]): A list of names, keywords to search for within the codebase.
            Terms can be formatted as 'file_path:QualifiedName' to search for a specific module or entity within a file
            (e.g., 'src/helpers/math_helpers.py:MathUtils.calculate_sum') or unqualified names to search for occurrences anywhere within the codebase.

    Returns:
        str: The search results, which are the locations of matching entities.

    Example Usage:
        # Search for the location of a specific file
        result = locate_entities(search_terms=['my_file.py'])

        # Search for the location of a specific class
        result = locate_entities(search_terms=['MyClass'])
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
                "start_line": node.start.line,
                "end_line": node.end.line,
            }
            for node in nodes
        ],
        indent=2,
    )


# @mcp.tool()
def traverse_graph(
    start_entities: list[str],
    direction: str = "downstream",
    traversal_depth: int = 1,
    entity_type_filter: list[str] | None = None,
    relationship_type_filter: list[str] | None = None,
) -> str:
    """Analyzes and displays the relationship structure around specified entities in a code graph.

    This function searches and presents relationships for the specified entities (such as classes, functions, files, or directories) in a code graph.
    It explores how the input entities relate to others, using defined types of relationships, including 'contains', 'imports', 'references' and 'inherits'.

    Example Usage:
    1. Exploring Outward Dependencies:
        ```
        traverse_graph(
            start_entities=['src/module_a.py:ClassA'],
            direction='downstream',
            traversal_depth=1,
            entity_type_filter=['class'],
            relationship_type_filter=['inherits']
        )
        ```
        This retrieves the relationships of `ClassA` up to 1 level deep, focusing only on what classes `ClassA` inherits, i.e., its parent classes.

    2. Exploring Inward Dependencies:
        ```
        traverse_graph(
            start_entities=['src/module_b.py:FunctionY'],
            direction='upstream',
            traversal_depth=-1
        )
        ```
        This finds all entities that contain/inherit/import/reference `FunctionY` without restricting the traversal depth.

    Notes:
    * Traversal Control: The `traversal_depth` parameter specifies how deep the function should explore the graph starting from the input entities.
    * Filtering: Use `entity_type_filter` and `relationship_type_filter` to narrow down the scope of the search, focusing on specific entity types and relationships.
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
        - 'downstream': Traversal to explore relationships from the specified entities to the candidate entities.
        - 'upstream': Traversal to explore relationships from the candidate entities to the specified entities.
        Default is 'downstream'. Specially for the 'inherits' relationship, use 'downstream' to find the parent class and 'upstream' to find the child classes.

    traversal_depth : int, optional
        Maximum depth of traversal. A value of -1 indicates unlimited depth (subject to a maximum limit).
        Must be either `-1` or a non-negative integer (≥ 0).
        Default is 1.

    entity_type_filter : list[str], optional
        List of entity types (e.g., 'class', 'function', 'file', 'directory') to include in the traversal.
        If None, all entity types are included.
        Default is None.

    relationship_type_filter : list[str], optional
        List of relationship types (e.g., 'contains', 'imports', 'references', 'inherits') to include in the traversal.
        If None, all relationship types are included.
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
            relationship_type_filter,
        )
        for node in start_entities
    }
    rtn_str = json.dumps(rtns)
    return rtn_str.strip()


@mcp.tool()
def search_parent_or_child_entities(entity_names: list[str], direction: str) -> str:
    """
    Searches for entities that are the parent classes of the given entity name.

    Args:
        entity_names (List[str]): List of entity names in a full-qualified format (e.g., 'src/module_a.py:ClassA').
        direction (str): The direction of the search. Can be 'parent' or 'child'.

    Returns:
        str: All the parent or child class entities.
    """
    direction = "downstream" if direction == "parent" else "upstream"  # type: ignore
    return traverse_graph(
        start_entities=entity_names,
        direction=direction,
        traversal_depth=1,
        entity_type_filter=["class", "unparsed"],
        relationship_type_filter=["inherits"],
    )


@mcp.tool()
def search_reference_entities(entity_name: str) -> str:
    """
    Searches for entities that reference (imports/inherits/references) the given entity name.

    Args:
        entity_name (List[str]): The entity name in a full-qualified format (e.g., 'src/module_a.py:ClassA').

    Returns:
        str: All the matching entities.
    """
    result = db.execute(
        """
        MATCH (a)<-[b:IMPORTS|:INHERITS|:REFERENCES]-(c)
        WHERE a.name = $entity_name
        RETURN c;
        """,
        parameters={"entity_name": entity_name},
    )
    if not result:
        return {}

    entities: list[str] = []
    for r in result:
        data = r[0]
        name = f"{data['name']}"
        if data["type"] in (NodeType.CLASS, NodeType.FUNCTION, NodeType.VARIABLE):
            name = f"{name}#L{data['start_line']}-L{data['end_line']}"
        entities.append(name)
    return json.dumps(entities, indent=2)


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
