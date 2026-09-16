# Multi-Agent Usage Guide

Factum is designed as a **shared auditable knowledge base** that multiple
AI agents can read from and write to simultaneously. This guide covers the
patterns, capabilities, and limitations when using Factum in multi-agent
scenarios.

## Design Position

Factum is a **memory layer**, not an agent framework. It does not handle:

- Agent orchestration or task assignment (use LangGraph, CrewAI, etc.)
- Message passing between agents (use your message bus of choice)
- Agent identity management or capability declarations
- Process scheduling or resource allocation

What Factum **does** provide is a structured, auditable knowledge store
where multiple agents can write facts with full provenance, and any agent
can query the shared knowledge base with confidence that the results are
traceable, permission-filtered, and conflict-aware.

## Architecture: Current Limitations

With current stdio transport, **each MCP client starts its own Factum
process**. There is no way to share a single Factum server instance
across multiple clients:

```
Agent A (MCP client) → Factum Process A → separate in-memory store
Agent B (MCP client) → Factum Process B → separate in-memory store
Agent C (MCP client) → Factum Process C → separate in-memory store
```

To share data, use RocksDB persistence (`--db-path`), but **only one
process can hold the RocksDB lock at a time**. A second process will
fail to open the database. There is no read-only mode — RocksDB
acquires an exclusive lock on open.

**Future** (ROADMAP M2): Streamable HTTP transport will allow a single
Factum server instance to serve multiple MCP clients over the network,
with proper MVCC concurrency control. See
[HTTP transport design](http-transport-design.md).

### Workaround for multi-agent today

1. **Coordinator pattern**: A single agent (or an external script)
   collects facts from all agents and writes them via `factum_insert_batch`.
   Other agents cannot directly access the store.

2. **Sequential writing**: Agents take turns starting a Factum process,
   writing their data, and shutting down. Use a coordination mechanism
   outside Factum (e.g., a file lock, a message queue) to ensure
   only one agent writes at a time.

3. **Export/import**: One agent writes and exports nodes (via
   `factum_search`), another agent imports them into its own store.

## Core Capabilities for Multi-Agent

### 1. Provenance — Who Wrote What

Every node carries mandatory provenance. In multi-agent scenarios, this
answers the critical question: *"Which agent wrote this fact, and should
I trust it?"*

```scheme
; Agent A extracts from a document
(node n001
  :pred (revenue @ACME-CORP 23050000000)
  :src (extracted "earnings-2024" [120 350] (model "claude-sonnet-4" "2025-01"))
  :conf 0.80)

; Agent B directly asserts based on its knowledge
(node n002
  :pred (ceo-of @FOUNDER-1 @ACME-CORP)
  :src (asserted "agent-b")
  :conf 0.60)
```

The `Principal` field in `Asserted` provenance and the `ModelRef` in
`Extracted` provenance provide the agent identity trail. When querying,
the caller can see exactly which agent or model produced each fact.

### 2. Permission Isolation — Who Can See What

Factum's index-level permission filtering ensures agents only see nodes
they're authorized to access. There is no post-query leakage.

```scheme
; Agent A writes a confidential analysis
(node n003
  :pred (risk-assessment @ACME-CORP "high-leverage-debt")
  :src (asserted "agent-a")
  :perm confidential
  :conf 0.70)

; Agent B (public-only context) queries — n003 is invisible
; Agent C (confidential context) queries — n003 is visible
```

Permission levels: `public`, `internal`, `confidential`, `restricted`.
Each agent's `PermissionContext` determines visibility at the index level.

### 3. Content-Addressed Deduplication — Corroboration

When multiple agents independently assert the same fact, Factum's
content-addressed node ID (`factum_assert`) naturally deduplicates them:

```
Agent A: factum_assert "(status @PROJECT active)" :by "agent-a"
→ auto-abc123def456 (inserted)

Agent B: factum_assert "(status @PROJECT active)" :by "agent-b"
→ auto-abc123def456 (AlreadyExists — same content, same ID)
```

This is a feature, not a bug: the second insert fails because the
knowledge already exists. The caller knows that another agent already
recorded this fact, which is a form of **implicit corroboration**.

For **explicit corroboration** (different confidence or provenance for
the same predicate), use `factum_insert` with a custom node ID:

```scheme
; Agent A's version
(node agent-a-001 :pred (status @PROJECT active) :src (asserted "agent-a") :conf 0.70)

; Agent B independently confirms
(node agent-b-001 :pred (status @PROJECT active) :src (asserted "agent-b") :conf 0.80)
```

### 4. Cascade Retraction — Error Propagation

When an agent discovers its earlier knowledge was wrong, retracting the
source node automatically invalidates all derived nodes — across all
agents that depended on it:

```
Agent A writes: (node n001 :pred (revenue @X 1000) ...)
Agent B derives: (node n002 :pred (growth-rate @X 0.15) :src (derived n001 "rule-001") :deps [n001])

Agent A discovers error → retract n001
→ n002 is automatically cascade-retracted
→ Agent B's derived knowledge is invalidated
```

This is critical for multi-agent correctness: an error in one agent's
input doesn't silently propagate to other agents' outputs.

### 5. WeightedVote Conflict Resolution

When multiple agents disagree on the same fact, the `WeightedVote`
policy resolves by weighted majority:

```json
{
  "query": "(status @PROJECT ?s)",
  "policy": "weighted",
  "agent_weights": {
    "agent-a": 0.5,
    "agent-b": 0.3,
    "agent-c": 0.2
  }
}
```

- Each agent's nodes are grouped by predicate value
- Weights are summed per group
- A group wins if its total weight exceeds 50% of total
- If no majority, returns `Ambiguous` (refuses to answer)

**Weight derivation**: Weights can be assigned based on agent reliability
(future M3: empirical reliability table), agent role (domain expert gets
higher weight), or manual configuration.

**Default weight**: Agents not in the weights map receive 0.5.

## What Factum Does NOT Do

| Capability | Status | Alternative |
|------------|--------|-------------|
| Agent orchestration | Not planned | LangGraph, CrewAI, AutoGen |
| Message passing | Not planned | RabbitMQ, Redis Pub/Sub, gRPC |
| Agent identity/registration | Not planned | Agent framework's responsibility |
| Concurrent multi-process writes | M2 (MVCC) | Single-writer pattern today |
| Semantic similarity search | Not planned | Mem0, Zep alongside Factum |
| Memory consolidation | Not planned | Letta alongside Factum |

## Multi-Agent Conflict Scenarios

### Scenario 1: Two agents disagree on a fact

```
Agent A: (status @SERVER-1 healthy) :conf 0.70
Agent B: (status @SERVER-1 degraded) :conf 0.80
```

With `LatestWins` policy: Agent B wins (higher authority).
With `Unanimous` policy: `Ambiguous` (refuses to answer).
With `WeightedVote` policy: resolves by weights (e.g., if A=0.6, B=0.4 → A wins).

### Scenario 2: One agent extracts, another verifies

```
Agent A: (revenue @ACME-CORP 23050000000) :src (extracted "doc-001" ...) :conf 0.80
Agent B: (revenue @ACME-CORP 23050000000) :src (asserted "agent-b") :conf 0.90
```

Both agree on the value. With `Unanimous`, this resolves (same predicate).
With `LatestWins`, Agent B wins (higher authority). The provenance chain
shows both the extraction source and the verification.

### Scenario 3: Cascade invalidation across agents

```
Agent A: (acquired-by @COMPANY-X @COMPANY-Y) :src (extracted "news-001" ...)
Agent B: (subsidiary-of @COMPANY-Y @COMPANY-X) :src (derived n001 "rule-001") :deps [n001]

Later: news-001 is retracted as unreliable
→ Agent A retracts n001
→ Agent B's n002 is cascade-retracted automatically
```

No explicit coordination needed — the dependency graph handles propagation.

## Best Practices

1. **Always include `:by` in `factum_assert`**: Set it to your agent name
   so provenance is traceable: `factum_assert "(status @X active)" :by "agent-a"`.

2. **Use `:perm` for sensitive data**: Agent-specific analysis should use
   `:perm confidential` or `:perm internal` to prevent other agents from
   seeing it prematurely.

3. **Set realistic `:conf`**: Don't set 1.0 unless you're formally certain.
   Provenance-based defaults are applied automatically when `:conf` is omitted.

4. **Use `:deps` for derived facts**: If your agent's fact depends on another
   agent's fact, list it in `:deps` so cascade retraction works.

5. **Choose the right conflict policy**:
   - `latest` (default): Best for single-agent or authoritative-source scenarios
   - `unanimous`: Best when all sources must agree (high-stakes decisions)
   - `weighted`: Best for multi-agent with known reliability differences

6. **Don't use high `min_conf` on mixed-era knowledge**: See
   [Confidence Calibration Guide](confidence-calibration.md) §Mixed-Era
   Knowledge Bases for the old-node-conf-inflation issue.

## Scaling to 10+ Agents

When deploying Factum with many agents, additional considerations apply.

### Trust tiers

Classify agents into trust tiers and assign weights accordingly:

| Tier | Weight | Examples |
|------|--------|----------|
| High trust | 0.7-1.0 | Verified extractors, formal verifiers, human-curated |
| Medium trust | 0.3-0.6 | LLM extractors with known model accuracy |
| Low trust | 0.1-0.2 | Unverified sources, experimental agents |

With WeightedVote, a single high-trust agent can outvote multiple
low-trust agents — preventing noise from overwhelming signal.

### Cascade depth protection

Always set `max_cascade_depth` when retracting in large knowledge bases:

```json
{"tool": "factum_retract", "arguments": {
  "node_id": "auto-xxx",
  "max_cascade_depth": 50
}}
```

Check the `truncated` field in the response. If `true`, some downstream
nodes were NOT retracted — investigate and retract them manually.

**Recommended defaults**:
- Small knowledge base (<100 nodes): 100 (default)
- Medium (100-10K nodes): 50
- Large (>10K nodes): 20

### Garbage metadata defense

In large-scale deployments, low-quality agents may produce noise. Defense
in depth:

1. **min_conf threshold**: Query with `min_conf: 0.5` to filter low-confidence
   assertions from unreliable agents.

2. **Permission isolation**: Assign low-trust agents a `public`-only
   permission context. Their outputs are visible but cannot contaminate
   `confidential` or `restricted` knowledge.

3. **WeightedVote policy**: Use `weighted` instead of `latest` for queries
   in multi-agent contexts. This ensures noise from many low-trust agents
   doesn't override a single high-trust assertion.

4. **Provenance audit**: Periodically query by principal to identify
   agents producing retracted or low-confidence nodes:
   ```
   factum_search mode="stats" → check retracted ratio per source
   ```

### Write coordination

With stdio transport (current), only one process can hold the RocksDB
write lock. Patterns:

- **Coordinator pattern**: One agent (or an external script) collects
  facts from all agents and writes them via `factum_insert_batch`.
- **Sequential writes**: Agents take turns writing, coordinated by an
  external lock (file lock, Redis lock, etc.).
- **Read-heavy pattern**: Most agents only query (read-only is safe
  concurrently). Designate one writer agent.

Future (ROADMAP M2): HTTP transport + MVCC will allow true concurrent
multi-agent writes.
