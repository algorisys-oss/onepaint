@AGENTS.md

## Claude Code specifics

- `AGENTS.md` is the single source of truth for the project. Put shared guidance
  there, not here. This file holds only Claude-specific notes.
- To look things up in happypaint, use the repograph tools (`find_symbol`,
  `node`, `explore`) with `path: ../happypaint`, or read
  `../happypaint/.repograph/index.txt`. This beats grepping the 107k-line TS
  tree.
- When porting a subsystem, delegate the happypaint read-through to an Explore
  agent and ask it for the API surface, invariants and tests. Keep the Rust
  implementation in the main session.
- Don't edit anything under `../happypaint`. It is the read-only spec.
