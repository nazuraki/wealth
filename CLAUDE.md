## Code intelligence

This project is indexed in nazu's code graph as `code:wealth`.

At the start of every session, call `get_project_overview("wealth")` via the
`code-graph` MCP server to orient yourself. Use `find_symbol`, `find_callers`,
and `query_code_graph` for targeted lookups — do not read source files to answer
structural questions.

Example queries:
- "What functions exist in the auth module?" → find_symbol("wealth", "auth")
- "Who calls saveUser?" → find_callers("wealth", "saveUser")
- "What services does this project use?" → get_project_overview("wealth")
