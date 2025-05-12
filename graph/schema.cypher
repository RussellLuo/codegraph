// Create nodes
CREATE NODE TABLE IF NOT EXISTS Unparsed (
    name STRING,
    type STRING,
    short_names STRING[],
    PRIMARY KEY(name)
);
CREATE NODE TABLE IF NOT EXISTS Directory (
    name STRING,
    type STRING,
    short_names STRING[],
    PRIMARY KEY(name)
);
CREATE NODE TABLE IF NOT EXISTS File (
    name STRING,
    type STRING,
    short_names STRING[],
    code STRING,
    PRIMARY KEY(name)
);
CREATE NODE TABLE IF NOT EXISTS Class (
    name STRING,
    type STRING,
    short_names STRING[],
    code STRING,
    start_line UINT32,
    end_line UINT32,
    PRIMARY KEY(name)
);
CREATE NODE TABLE IF NOT EXISTS Function (
    name STRING,
    type STRING,
    short_names STRING[],
    code STRING,
    start_line UINT32,
    end_line UINT32,
    PRIMARY KEY(name)
);

// Create relationships
CREATE REL TABLE IF NOT EXISTS CONTAINS (
    From Directory To Directory,
    From Directory To File,
    From File To Class,
    From File To Function,
    type STRING
);
CREATE REL TABLE IF NOT EXISTS IMPORTS (
    From File To File,
    From File To Class,
    From File To Function,
    From File To Unparsed,
    type STRING,
    import STRING,
    alias STRING
);

CREATE REL TABLE IF NOT EXISTS INHERITS (
    From Class To Unparsed,
    From Class To Class,
    type STRING
);
