# Versioning

## Current Scheme

bexp follows [Semantic Versioning](https://semver.org/). The project is pre-1.0 (`0.x.y`), meaning the API is not yet stable and minor version bumps may include breaking changes.

## Public API Surface

The MCP tool interface is the public API. The following are considered breaking changes:

- Removing or renaming an MCP tool
- Removing or renaming a tool parameter
- Changing the semantics of an existing parameter
- Changing response structure in a way that breaks existing consumers

## Tool Naming Convention

Tools use **snake_case** with a `verb_noun` pattern:

- `get_context_capsule`
- `search_memory`
- `get_skeleton`
- `reindex_workspace`

New tools must follow this convention.

## Version Bump Policy

| Change | Bump |
|---|---|
| New tool added | Minor (`0.x+1.0`) |
| New optional parameter on existing tool | Minor |
| Tool removed or renamed | Minor (pre-1.0), Major (post-1.0) |
| Parameter removed or renamed | Minor (pre-1.0), Major (post-1.0) |
| Bug fix, performance improvement | Patch (`0.x.y+1`) |

## Future: Structured Responses

All tools currently return markdown strings. A future version will introduce an opt-in `response_format` parameter (`"markdown"` | `"json"`) to allow clients to request structured JSON responses. The default will remain `"markdown"` for backwards compatibility.
