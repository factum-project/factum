# Multi-Agent Conflict Resolution Demo

This demo shows how multiple AI agents can use Factum as a shared auditable
knowledge base — with provenance tracking, conflict resolution, and cascade
retraction. **No new code is needed** — this script uses only existing MCP
tools (`factum_assert`, `factum_query`, `factum_retract`).

## Scenario

Three agents (analyst-a, analyst-b, analyst-c) are researching the same
company. They disagree on the company's financial health. The demo shows:

1. **Independent assertions** — each agent writes facts with provenance
2. **Conflict detection** — query reveals conflicting values
3. **WeightedVote resolution** — weighted majority resolves the conflict
4. **Ambiguous refusal** — when no majority exists, Factum refuses to answer
5. **Cascade retraction** — retracting a source invalidates all derived facts

## Prerequisites

- Factum MCP server binary (`cargo build --release -p factum-mcp`)
- Any MCP-compatible client (Claude Code, Cursor, or a script)

## Step-by-step script

### Step 1: Agents write independent facts

**Agent A** (financial analyst, high trust) asserts revenue is declining:

```json
{"tool": "factum_assert", "arguments": {
  "text": "(revenue-trend @ACME-CORP declining)",
  "by": "analyst-a",
  "conf": 0.80
}}
```

**Agent B** (junior analyst, lower trust) asserts revenue is growing:

```json
{"tool": "factum_assert", "arguments": {
  "text": "(revenue-trend @ACME-CORP growing)",
  "by": "analyst-b",
  "conf": 0.60
}}
```

**Agent C** (sector specialist, medium trust) agrees with Agent A:

```json
{"tool": "factum_assert", "arguments": {
  "text": "(revenue-trend @ACME-CORP declining)",
  "by": "analyst-c",
  "conf": 0.75
}}
```

### Step 2: Query reveals conflict

```json
{"tool": "factum_query", "arguments": {
  "query": "(revenue-trend @ACME-CORP ?trend)"
}}
```

Response shows two conflicting values: `declining` (2 agents) and `growing`
(1 agent). The default `latest` policy picks one, but doesn't reflect
agent trust.

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

Result: `declining` wins (weight 0.8 vs 0.2 — 80% majority).

How it works:
- Agent A and C both asserted `declining` → combined weight 0.5 + 0.3 = 0.8
- Agent B asserted `growing` → weight 0.2
- Total weight = 1.0, majority threshold = 0.5
- 0.8 > 0.5 → `declining` wins

### Step 4: Ambiguous refusal

Now suppose Agent C changes their mind and also says `growing`:

```json
{"tool": "factum_assert", "arguments": {
  "text": "(revenue-trend @ACME-CORP growing)",
  "by": "analyst-c",
  "conf": 0.75
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
out to be erroneous. Retracting the source fact cascades to all derived
facts:

First, Agent A wrote a derived fact based on the revenue trend:

```json
{"tool": "factum_insert", "arguments": {
  "node": "(node n-risk-001 :pred (risk-level @ACME-CORP high) :src (derived auto-aaa-001 \"rule-revenue-decline\") :deps [auto-aaa-001] :conf 0.70)"
}}
```

Now retract the source:

```json
{"tool": "factum_retract", "arguments": {
  "node_id": "auto-aaa-001",
  "max_cascade_depth": 50
}}
```

Response:

```json
{
  "retracted": ["auto-aaa-001", "n-risk-001"],
  "count": 2,
  "truncated": false,
  "depth_reached": 1
}
```

Both the source fact and the derived risk assessment are retracted —
across all agents. The `truncated: false` confirms the cascade completed
within the depth limit.

## What this demo proves

| Capability | Demonstrated in |
|------------|----------------|
| Provenance tracking | Steps 1-2: each fact shows which agent wrote it |
| Content-addressed dedup | Step 1: Agent C's duplicate assertion gets same ID |
| Conflict detection | Step 2: query shows conflicting values |
| WeightedVote arbitration | Step 3: weighted majority resolves conflict |
| Ambiguous refusal | Step 4: Factum refuses when no majority |
| Cascade retraction | Step 5: source error propagates to derived facts |
| Cascade depth limit | Step 5: `max_cascade_depth` prevents explosion |

## Scaling notes

- **10 agents**: Works today with sequential writes via a coordinator
- **100 agents**: Requires HTTP transport (ROADMAP M2) for concurrent access
- **1000 agents**: Requires MVCC + HTTP transport (ROADMAP M2)

The governance model (provenance + arbitration + cascade) scales to any
number of agents — the limitation is transport, not logic.
