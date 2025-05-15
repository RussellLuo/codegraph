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
CREATE NODE TABLE IF NOT EXISTS Variable (
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
    From File To Variable,
    From Class To Function,
    type STRING
);
CREATE REL TABLE IF NOT EXISTS IMPORTS (
    From File To File,
    From File To Class,
    From File To Function,
    From File To Variable,
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
CREATE REL TABLE IF NOT EXISTS REFERENCES (
    From Class To Class,
    From Class To Function,
    From Class To Variable,
    From Class To Unparsed,
    From Function To Class,
    From Function To Function,
    From Function To Variable,
    From Function To Unparsed,
    From Variable To Class,
    From Variable To Function,
    From Variable To Variable,
    From Variable To Unparsed,
    type STRING
);
