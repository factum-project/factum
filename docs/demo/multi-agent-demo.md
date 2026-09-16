# Multi-Agent Conflict Resolution Demo

This demo shows how multiple AI agents can use Factum as a shared auditable
knowledge base — with provenance tracking, conflict resolution, and cascade
retraction. **No new code is needed** — this script uses only existing MCP
tools (`factum_assert`, `factum_query`, `factum_retract`).

## Scenario

Three agents (analyst-a, analyst-b, analyst-c) are researching the same
company. They disagree on the company's financial health. The demo shows:

1. **Independent assertions** — each agent writes facts with provenance
2. **Corroboration** — same fact from different agents is detected, not rejected
3. **Conflict detection** — query reveals conflicting values
4. **WeightedVote resolution** — weighted majority resolves the conflict
5. **Ambiguous refusal** — when no majority exists, Factum refuses to answer
6. **Cascade retraction** — retracting a source invalidates all derived facts

## Prerequisites

- Factum MCP server binary (`cargo build --release -p factum-mcp`)
- Any MCP-compatible client (Claude Code, Cursor, or a script)

## Important: parameter names

The `factum_assert` tool uses `predicate` (not `text`) and `confidence`
(accepts `conf` as an alias). Unknown fields are rejected (not silently
ignored). Example of correct call:

```json
{
  "tool": "factum_assert",
  "arguments": {
    "predicate": "(revenue-trend @ACME-CORP declining)",
    "by": "analyst-a",
    "confidence": 0.80
  }
}
```

## Step-by-step script

### Step 1: Agents write independent facts

**Agent A** (financial analyst, high trust) asserts revenue is declining:

```json
{"tool": "factum_assert", "arguments": {
  "predicate": "(revenue-trend @ACME-CORP declining)",
  "by": "analyst-a",
  "confidence": 0.80
}}
```

Response:
```json
{
  "action": "asserted",
  "node_id": "auto-<12 hex chars>",
  "predicate": "(revenue-trend @ACME-CORP declining)",
  "status": "fact asserted with auto-generated ID"
}
```

**Note the `node_id`** — you'll need it for Step 6. The actual ID is a
content hash (e.g., `auto-9a3f7b2c1e8d`), not a placeholder.

**Agent B** (junior analyst, lower trust) asserts revenue is growing:

```json
{"tool": "factum_assert", "arguments": {
  "predicate": "(revenue-trend @ACME-CORP growing)",
  "by": "analyst-b",
  "confidence": 0.60
}}
```

This succeeds (different predicate → different content hash → different ID).

**Agent C** (sector specialist, medium trust) agrees with Agent A:

```json
{"tool": "factum_assert", "arguments": {
  "predicate": "(revenue-trend @ACME-CORP declining)",
  "by": "analyst-c",
  "confidence": 0.75
}}
```

Response:
```json
{
  "action": "corroborated",
  "node_id": "auto-<same hash as Agent A>",
  "predicate": "(revenue-trend @ACME-CORP declining)",
  "status": "fact already exists — your assertion is recorded as corroboration",
  "corroborated_by": "analyst-a",
  "your_principal": "analyst-c"
}
```

Agent C's assertion returns `corroborated` (not an error). The content-addressed
ID detects that the same fact was already asserted by a different principal.
The hint explains how to add a separate node with different provenance if
needed for WeightedVote.

### Step 2: Query reveals conflict

```json
{"tool": "factum_query", "arguments": {
  "query": "(revenue-trend @ACME-CORP ?trend)"
}}
```

With the default `latest` policy, the query returns the node with the highest
authority. Two conflicting values exist: `declining` and `growing`.

### Step 3: WeightedVote resolution

```json
{"tool": "factum_query", "arguments": {
  "query": "(revenue-trend @ACME-CORP ?trend)",
  "policy": "weighted",
  "agent_weights": {
    "analyst-a": 0.5,
    "analyst-b": 0.2,
    "analyst-c": 0.3
  }
}}
```

Result: `declining` wins.

How it works:
- Agent A asserted `declining` → weight 0.5
- Agent B asserted `growing` → weight 0.2
- Agent C corroborated `declining` (but since it's the same content-addressed
  node, only Agent A's provenance is stored in the node)
- The `declining` group has weight 0.5, the `growing` group has weight 0.2
- Total weight = 0.7, majority threshold = 0.35
- 0.5 > 0.35 → `declining` wins

**Note**: For multi-principal corroboration to count in WeightedVote, each
agent must use `factum_insert` with a custom node ID (not `factum_assert`,
which deduplicates by content hash). See the
[multi-agent usage guide](../multi-agent-usage.md) §Explicit Corroboration.

### Step 4: Ambiguous refusal

Now suppose Agent C changes their mind and also says `growing` (using
`factum_insert` with a custom ID to add their own provenance):

```json
{"tool": "factum_insert", "arguments": {
  "node": "(node corr-c-001 :pred (revenue-trend @ACME-CORP growing) :src (asserted \"analyst-c\") :conf 0.75)"
}}
```

Now the split is: `declining` (A, weight 0.5) vs `growing` (B+C, weight 0.5).

```json
{"tool": "factum_query", "arguments": {
  "query": "(revenue-trend @ACME-CORP ?trend)",
  "policy": "weighted",
  "agent_weights": {
    "analyst-a": 0.5,
    "analyst-b": 0.2,
    "analyst-c": 0.3
  }
}}
```

Result: **Ambiguous** — no group exceeds 50%. Factum refuses to answer
rather than returning a potentially wrong result.

### Step 5: Cascade retraction

Agent A's original assertion was based on an earnings report that turned
out to be erroneous. First, a derived fact was created based on the
revenue trend (use the actual `node_id` from Step 1):

```json
{"tool": "factum_insert", "arguments": {
  "node": "(node n-risk-001 :pred (risk-level @ACME-CORP high) :src (derived auto-<actual-id-from-step-1> \"rule-revenue-decline\") :deps [auto-<actual-id-from-step-1>] :conf 0.70)"
}}
```

Now retract the source (replace `auto-xxx` with the actual ID from Step 1):

```json
{"tool": "factum_retract", "arguments": {
  "node_id": "auto-<actual-id-from-step-1>",
  "max_cascade_nodes": 50
}}
```

Response:

```json
{
  "retracted": ["auto-<actual-id>", "n-risk-001"],
  "count": 2,
  "truncated": false,
  "depth_reached": 1
}
```

Both the source fact and the derived risk assessment are retracted.
The `truncated: false` confirms the cascade completed within the node limit.

## What this demo proves

| Capability | Demonstrated in |
|------------|----------------|
| Provenance tracking | Steps 1-2: each fact shows which agent wrote it |
| Corroboration detection | Step 1: Agent C's same-fact assertion returns `corroborated` |
| Conflict detection | Step 2: query shows conflicting values |
| WeightedVote arbitration | Step 3: weighted majority resolves conflict |
| Ambiguous refusal | Step 4: Factum refuses when no majority |
| Cascade retraction | Step 5: source error propagates to derived facts |
| Cascade node limit | Step 5: `max_cascade_nodes` prevents explosion |

## Scaling notes

- **10 agents**: Works today with sequential writes via a coordinator
- **100 agents**: Requires HTTP transport (ROADMAP M2) for concurrent access
- **1000 agents**: Requires MVCC + HTTP transport (ROADMAP M2)

The governance model (provenance + arbitration + cascade) scales to any
number of agents — the limitation is transport, not logic.
