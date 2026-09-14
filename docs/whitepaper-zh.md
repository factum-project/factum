# Factum 技术白皮书

**面向 LLM 的原生知识语言**

> **版本：v0.1.0 — 工作草案，寻求早期合作者**
> **日期：2026 年 9 月**
> **许可证：MIT**

---

## 目录

1. [引言](#1-引言)
2. [三层愿景](#2-三层愿景)
3. [Node 七元组数据模型](#3-node-七元组数据模型)
4. [Factum-F 语法](#4-factum-f-语法)
5. [无损数值表示](#5-无损数值表示)
6. [五级溯源体系](#6-五级溯源体系)
7. [时间有效性](#7-时间有效性)
8. [冲突仲裁](#8-冲突仲裁)
9. [索引级权限过滤](#9-索引级权限过滤)
10. [级联撤回](#10-级联撤回)
11. [双序列化格式与 Token 效率](#11-双序列化格式与-token-效率)
12. [MCP 集成](#12-mcp-集成)
13. [词素词表系统](#13-词素词表系统)
14. [解析器与 DoS 防护](#14-解析器与-dos-防护)
15. [验证器框架](#15-验证器框架)
16. [查询引擎](#16-查询引擎)
17. [项目架构](#17-项目架构)
18. [路线图](#18-路线图)
19. [与其他格式的关系](#19-与其他格式的关系)
20. [风险与缓解](#20-风险与缓解)

---

## 1. 引言

### 1.1 问题

大语言模型（LLM）在处理结构化知识时面临三个根本性瓶颈：

- **读取瓶颈**：LLM 接收的知识上下文以自然语言文本（Markdown、JSON）为主，缺乏内置的溯源、置信度和时间有效性信息。模型无法区分"维基百科的原文引用"与"某 LLM 的推断提取"。

- **写入瓶颈**：LLM 生成的结构化输出（JSON、函数调用）缺少可验证的语法约束。语法歧义导致解析不确定性，错误信息缺乏分类，模型无法自我纠正。

- **推理瓶颈**：LLM 的"思考"发生在连续向量空间中，知识以隐式权重参数形式存储。没有结构化的知识表示可供 LLM 进行可审计的推理——你无法回答"模型为什么在日期 Y 相信 X"。

### 1.2 Factum 的定位

Factum 是一种**结构化知识表示语言**，设计目标不是成为人类面向的数据库格式，而是成为 **LLM 的原生知识媒介**：

- LLM 将 Factum-F 作为上下文**读取**
- LLM 将 Factum-F 作为输出**生成**
- （远期）LLM 在 Factum-F 的潜空间投影中**推理**

每一个设计决策都服务于 LLM 原生使用场景：全括号化保证 LLM 生成的语法确定性，错误分类体系支持 LLM 自我纠正，紧凑格式节省上下文窗口，`Dec(i128, u8)` 捕获 LLM 的数值幻觉，提取级知识强制携带模型引用以支持 LLM 自审计。

### 1.3 当前状态

| 模块 | 状态 | 说明 |
|------|------|------|
| factum-core（类型、词法器、解析器、序列化） | ✅ 已实现 | 100% 语法往返一致性 |
| factum-rt（存储、查询、仲裁、权限、验证器） | ✅ 已实现 | 内存存储——RocksDB 后端为路线图项 |
| factum-mcp（JSON-RPC 桥接） | ✅ 协议层+处理器 | stdio 传输未验证，HTTP 传输未实现 |
| factum-bench（基准测试） | ✅ 已实现 | 语法往返 + token 效率 + 查询性能 |
| factum-l（潜空间投影） | ❌ 未开始 | 研究级，M3 里程碑 |

---

## 2. 三层愿景

Factum 的终局愿景分为三层，每一层解决 LLM 知识处理的一个核心瓶颈：

### 2.1 LLM Read（读取层）

LLM 通过 MCP（Model Context Protocol）接收 Factum-F 作为上下文负载。与冗长的 JSON 元数据相比，Factum 的规范 S-expression 形式在 token 消耗上具有显著优势——启发式估算显示规范形式比同等元数据的 JSON 节省约 66% 的 token。

**状态**：架构就绪，token 效率为启发式估算（真实测量追踪于 issue #9）。

### 2.2 LLM Write（写入层）

LLM 直接生成 Factum-F 节点。全括号化的 S-expression 语法保证每个合法输入有且仅有一个解析树——这意味着 LLM 生成的文本要么合法，要么不合法，不存在歧义解析。当生成不合法时，结构化的错误分类（`MissingPredField`、`UnknownNodeField`、`NamedArgBeforePositional` 等）使 LLM 能够理解错误类型并自我纠正。

**状态**：架构就绪，参见 [LLM 编写指南](authoring-for-llms.md)（草案）。

### 2.3 LLM Think（思考层）

factum-l 将 Factum-F 编码为连续思维向量 `z`，LLM 在 `z` 空间中推理，再解码回 Factum-F' 供审计。语义往返目标：v0.1 ≥ 0.95，验收阈值 ≥ 0.99。

**状态**：M3 研究项——未开始，不阻塞前两层。

---

## 3. Node 七元组数据模型

### 3.1 核心结构

Factum 中的每一条知识都是一个 `Node`，包含七个核心字段（七元组）加两个运行时元数据字段：

```rust
pub struct Node {
    // ── 七元组（核心知识字段）──
    pub id: NodeId,              // 1. 唯一标识符
    pub predicate: Predicate,    // 2. 内容谓词（断言什么）
    pub validity: Validity,      // 3. 时间有效性窗口
    pub provenance: Provenance,  // 4. 溯源链（知识来源）
    pub confidence: Confidence,  // 5. 置信度 [0, 1]
    pub authority: Authority,    // 6. 来源权威度 [0, 1]
    pub permissions: PermissionTag, // 7. 访问控制标签

    // ── 运行时元数据 ──
    pub deps: Vec<NodeId>,       // 依赖链（级联撤回用）
    pub status: NodeStatus,      // 生命周期状态
}
```

**七元组**：`(id, predicate, validity, provenance, confidence, authority, permissions) + deps + status`

### 3.2 设计原则

与 RDF 的三元组（subject, predicate, object）相比，Factum 的七元组将溯源、置信度、时间有效性和权限提升为**一等公民**——不是通过 reification 附加，而是每个节点原生携带。

| 字段 | RDF 中的处理 | Factum 中的处理 |
|------|-------------|----------------|
| 溯源 | 需要 reification（4 个额外三元组） | 原生字段，5 级分类 |
| 置信度 | 无标准表达 | 原生 f32 字段，[0, 1] |
| 时间有效性 | 需要 Time ontology 扩展 | 原生字段，支持窗口/永久 |
| 权限 | 无标准表达 | 原生位掩码，索引级过滤 |
| 权威度 | 无标准表达 | 原生 f32 字段，[0, 1] |

### 3.3 谓词结构

谓词是节点的内容核心：

```rust
pub struct Predicate {
    pub head: PredicateHead,     // 谓词头（词素 ID 或名称）
    pub args: Vec<Term>,         // 位置参数
    pub named: Vec<(SmolStr, Term)>, // 命名参数（必须在位置参数之后）
}
```

`Term` 支持五种类型：变量（`?p`）、实体引用（`@ACME-CORP`）、字面量（`0.73`）、复合谓词（嵌套 predicate）、列表。

**命名参数顺序约束**：命名参数必须出现在位置参数之后。这保证了语法解析唯一性——不存在前缀歧义。

---

## 4. Factum-F 语法

### 4.1 EBNF 语法

```text
node       := "(" "node" id node-body* ")"
node-body  := ":pred" predicate
             | ":valid" validity
             | ":src" provenance
             | ":conf" number
             | ":auth" number
             | ":deps" id-list
             | ":perm" tag
predicate  := "(" symbol term* named-arg* ")"
term       := var | entity | literal | predicate | list
literal    := number | string | date | duration | bool | uri
named-arg  := ":" symbol term
list       := "[" term* "]"
validity   := "forever" | "(" "window" date date? ")"
provenance := "(" "verbatim" doc span ")"
             | "(" "summary" doc span ")"
             | "(" "extracted" doc span model ")"
             | "(" "derived" from rule ")"
             | "(" "asserted" by ")"
```

### 4.2 语法示例

```scheme
; Acme Corp 知识图谱

; 节点 1：基础断言——Acme 是一个组织
(node n001
  :pred (instance-of @ACME-CORP organization)
  :conf 0.99 :auth 0.95 :perm public
  :src (asserted "wikidata"))

; 节点 2：LLM 提取——大股东信息
(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "doc002" [100 200] (model "gpt-4" "2024-06")))

; 节点 3：派生知识——子公司关系
(node n006
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :src (derived n001 "rule-subsidiary-merge")
  :deps [n001])
```

### 4.3 解析唯一性

两条规则保证每个合法输入有且仅有一个解析树：

1. **全括号化**：消除运算符优先级歧义。不存在中缀表达式，所有嵌套通过显式括号表达。
2. **命名参数顺序约束**：命名参数必须在位置参数之后。这消除了 `:keyword` 前缀与符号之间的歧义。

解析唯一性是整个系统的**信任基石**——如果解析不确定，往返测试就失去意义。

### 4.4 标识符字符集

v0.1-alpha 版本中，实体名称（`@Foo`）、符号和节点 ID 限制为 ASCII：`[a-zA-Z][a-zA-Z0-9_-]*`。Unicode 标识符支持和 NFC 规范化推迟到未来版本。

---

## 5. 无损数值表示

### 5.1 Dec(i128, u8)

Factum 中所有数值使用 `Dec(i128, u8)` 定点十进制表示——**零浮点误差**：

```rust
pub enum Literal {
    Dec(i128, u8),    // 尾数 × 10^(-scale)
    Str(SmolStr),
    Date(NaiveDate),
    Dur(std::time::Duration),
    Bool(bool),
    Uri(SmolStr),
}
```

`Dec(i128, u8)` 存储一个尾数（i128）和一个标度（u8, 0-255），表示 `mantissa × 10^(-scale)`。

**示例**：
- `230.50` → `Dec(23050, 2)`
- `0.001` → `Dec(1, 3)`
- `-5.25` → `Dec(-525, 2)`

### 5.2 为什么不用 f64？

浮点数引入跨平台不确定性（不同舍入模式、不同中间精度）。对于一个以可验证往返为核心价值主张的系统，这是不可接受的。

`Dec(i128, u8)` 提供：
- **精确十进制表示**（不存在 `0.1 + 0.2 ≠ 0.3` 问题）
- **跨平台确定性**
- **足够范围**（i128 支持约 38 位十进制数字，满足金融金额需求）

### 5.3 例外：置信度与权威度

`Confidence` 和 `Authority` 使用 `f32` 而非 `Dec`，因为它们是**主观度量**，不参与精确算术——你不会对置信度求和或计算权威度微分。用 `Dec` 表示 0.85 的置信度会暗示虚假的精度。

```rust
pub struct Confidence(pub f32);  // [0, 1]，构造时验证
pub struct Authority(pub f32);   // [0, 1]，构造时验证
```

---

## 6. 五级溯源体系

### 6.1 溯源分类

每条知识必须声明来源。Factum 定义五个溯源级别，每个级别承载不同的信任含义：

```rust
pub enum Provenance {
    Verbatim { doc: DocId, span: Span },                    // 原文逐字引用
    Summary { doc: DocId, span: Span },                     // 人工摘要
    Extracted { doc: DocId, span: Span, model: ModelRef },  // LLM 提取（必须携带模型引用）
    Derived { from: NodeId, rule: RuleId },                 // 形式化推导
    Asserted { by: Principal },                             // 直接断言
}
```

| 级别 | 含义 | 信任含义 | 必需字段 |
|------|------|---------|---------|
| **Verbatim** | 文档原文逐字引用 | 最高——可重建原文 | doc, span |
| **Summary** | 人工撰写的摘要 | 高——人类判断 | doc, span |
| **Extracted** | LLM 提取的知识 | 中——可能幻觉 | doc, span, **model**（必需） |
| **Derived** | 从其他节点形式化推导 | 取决于推导规则 | from, rule |
| **Asserted** | 人类或外部系统直接断言 | 取决于断言者 | by |

### 6.2 模型引用约束

`Extracted` 级别**必须**携带模型引用（名称 + 版本）：

```rust
pub struct ModelRef {
    pub name: SmolStr,    // e.g., "gpt-4"
    pub version: SmolStr, // e.g., "2024-06"
}
```

这一约束在**解析器中强制执行**，不仅在文档中声明。同一段文本被 GPT-4 和更小模型提取的可靠性截然不同——没有模型引用的提取知识是审计链的断裂。

### 6.3 设计动机

单一的 "source" 字段无法区分上述五种情况。一个 LLM 提取的"幻觉"事实和一个维基百科的原文引用不应以相同信任级别对待。五级溯源体系使信任评估细粒度化，并为级联撤回提供依赖链基础。

---

## 7. 时间有效性

### 7.1 有效性类型

```rust
pub enum Validity {
    Forever,                                    // 永久有效
    Window { from: DateTime<Utc>, until: Option<DateTime<Utc>> }, // 时间窗口
}
```

- `Forever`：节点始终有效（默认值）
- `Window { from, until: Some(...) }`：有界窗口，从 `from` 到 `until`
- `Window { from, until: None }`：开放窗口，从 `from` 起永久有效

### 7.2 语法表示

```scheme
:valid forever                                    ; 永久有效
:valid (window "2001-03-15T00:00:00+00:00" "2025-12-31T00:00:00+00:00")  ; 有界窗口
:valid (window "2001-03-15T00:00:00+00:00")      ; 开放窗口（无过期）
```

### 7.3 有效期检查

```rust
impl Validity {
    pub fn is_valid_at(&self, t: DateTime<Utc>) -> bool {
        match self {
            Validity::Forever => true,
            Validity::Window { from, until } => {
                *from <= t && until.is_none_or(|u| t < u)
            }
        }
    }
}
```

时间有效性支持"截至某时刻"的历史查询——查询返回在当时有效的知识，即使部分节点后来已过期或被撤回。

---

## 8. 冲突仲裁

### 8.1 问题

当多个节点匹配同一查询（相同实体、相同谓词、不同值）时，知识系统需要决定返回哪个答案。大多数系统默默选择一个——Factum 认为**不确定时拒绝猜测**比错误回答更好。

### 8.2 仲裁策略

```rust
pub enum ConflictPolicy {
    LatestWins,        // 最高权威度获胜，时间戳决胜
    HighestAuthority,  // 严格按权威度
    Unanimous,         // 仅当所有来源一致时返回
    Custom,            // 用户自定义（v0.1 未实现）
}
```

### 8.3 仲裁结果

```rust
pub enum ArbitrationResult {
    Resolved(Arc<Node>),          // 唯一胜出者
    Unanimous(Vec<Arc<Node>>),    // 多个来源一致
    Ambiguous(Vec<Arc<Node>>),    // 无法解决——拒绝回答
}
```

### 8.4 "拒绝猜测"原则

当仲裁策略无法唯一解决时，Factum 返回 `Ambiguous` 而非猜测。这是核心设计原则：

> 一个默默猜测的知识系统比一个承认不确定的系统更危险。

调用方可以决定是否调查、请求人工介入或应用不同策略。

### 8.5 仲裁流程

1. 将匹配结果按变量绑定签名分组
2. 组内只有 1 个结果 → 直接返回
3. 组内有多个结果 → 按策略仲裁
   - `LatestWins`：按权威度降序排序，返回最高者
   - `HighestAuthority`：严格按权威度，多个最高者则标记 `Ambiguous`
   - `Unanimous`：检查所有谓词是否相同，不同则标记 `Ambiguous`

---

## 9. 索引级权限过滤

### 9.1 问题

后查询过滤（检索所有匹配结果后过滤）是 RAG 系统中最常见的权限漏洞。问题在于聚合查询：即使用户看不到被过滤的个体结果，"查询返回 10 条但过滤掉 3 条"这一事实本身就泄露了机密节点的存在。

### 9.2 Factum 的解决方案

权限标签使用位掩码表示：

```rust
pub struct PermissionTag(pub u32);

impl PermissionTag {
    pub const PUBLIC: Self      = Self(0b0000_0001);
    pub const INTERNAL: Self    = Self(0b0000_0010);
    pub const CONFIDENTIAL: Self = Self(0b0000_0100);
    pub const RESTRICTED: Self  = Self(0b0000_1000);
}
```

权限过滤在**候选生成阶段**进行——查询者无权访问的节点永远不会被物化。在 v0.1 中，这通过候选枚举阶段的功能性过滤实现；在生产版本（RocksDB）中，将成为 `by_perm` 索引上的位图交集。

### 9.3 权限上下文

```rust
pub struct PermissionContext {
    pub principal: SmolStr,   // 查询者身份
    pub role_mask: u32,       // 角色位掩码
}
```

预定义上下文：`public()`（仅公开）、`internal()`（公开+内部）、`confidential()`（公开+内部+机密）、`admin()`（全部）。

---

## 10. 级联撤回

### 10.1 软删除

Factum 使用软删除——撤回的节点标记为 `Retracted` 但不删除：

```rust
pub enum NodeStatus {
    Active,     // 活跃且有效
    Retracted,  // 已撤回（软删除）
    Pending,    // 待验证
}
```

这保留了审计历史："截至日期 Y，我们为什么相信 X？" 即使上游来源后来被撤回，仍可查询过去的知识状态。

### 10.2 级联传播

当源节点被撤回时，所有通过 `Derived` 溯源依赖它的节点自动级联撤回。系统维护反向依赖图 `deps_rev`：

```rust
// 正向依赖：节点声明它依赖哪些节点
node.deps = [n001, n002]

// 反向依赖：存储维护 "谁依赖我" 的映射
deps_rev: HashMap<NodeId, Vec<NodeId>>
```

级联撤回流程：
1. 将目标节点标记为 `Retracted`
2. 查找 `deps_rev` 中依赖该节点的所有节点
3. 对每个依赖节点，如果其溯源为 `Derived` 且状态为 `Active`，递归撤回
4. 返回所有被撤回的节点 ID 列表

### 10.3 设计动机

硬删除会破坏审计链——如果上游来源被删除，你永远无法回答"为什么在日期 Y 相信 X"。级联撤回在保持审计完整性的同时自动维护知识一致性。

---

## 11. 双序列化格式与 Token 效率

### 11.1 两种格式

Factum 定义两种序列化格式，服务于不同场景：

| 格式 | 编码 | 用途 | 特征 |
|------|------|------|------|
| **规范形式** | S-expression | 哈希、往返一致性 | 全括号化，字段名完整 |
| **紧凑形式** | JSON | MCP 传输、存储 | 数字键替代字段名，词素索引 |

### 11.2 紧凑形式编码

紧凑形式用数字键替代冗长的字段名：

```json
{
  "0": "n001",                    // id
  "1": 42,                        // head: 词素索引或字符串名
  "2": ["@ACME-CORP", "0.73"],    // args
  "3": [["period", "#date(2024-01-01)"]],  // named
  "4": "forever",                 // validity
  "5": {"t":"extracted","v":{"d":"doc001","s":[0,100],"m":["gpt-4","2024-06"]}},  // provenance
  "6": 0.85,                      // confidence
  "7": 0.9,                       // authority
  "8": 4,                         // permission bitmask
  "9": ["n001", "n002"]           // deps
}
```

### 11.3 Token 效率发现

> **关键发现**：规范 S-expression 形式比紧凑 JSON 形式更省 token——这违反直觉。

| 格式 | 字节数（5 节点） | 估算 token 数 | vs JSON token 减少 |
|------|-----------------|-------------|-------------------|
| 紧凑 JSON | ~350 | ~350 | −12% |
| 规范 S-expr | 649 | ~134 | **−66%** |
| 冗长 JSON（基线） | 1448 | ~399 | — |

**原因**：BPE 分词器将 JSON 分隔符（`{`、`}`、`"`、`:`）拆分为独立 token，而 S-expression 的括号和空格经常与相邻 token 合并。这意味着为正确性设计的规范形式恰好也是 LLM 上下文窗口最 token 高效的形式。

> ⚠️ 以上 token 数据为启发式估算（±15% 接近 o200k_base），真实分词器测量追踪于 issue #9。

### 11.4 形式定位决策（待定）

如果真实分词器测量确认 token 差距，将触发形式重定位：

- **紧凑形式**重新定位为"存储/服务间格式"（字节最优）
- **规范形式**提供给 LLM 客户端（token 最优）
- 通过 `capabilities.factum.preferred_form` 协商返回格式

---

## 12. MCP 集成

### 12.1 JSON-RPC 2.0 桥接

Factum 通过 MCP（Model Context Protocol）与 LLM 客户端通信，使用 JSON-RPC 2.0 协议，兼容 MCP 2025-06-18 规范。

### 12.2 三个工具

| 工具 | 功能 | 必需参数 | 可选参数 |
|------|------|---------|---------|
| `factum_query` | 查询知识图谱 | `query`（S-expression） | `policy`, `as_of`, `min_confidence` |
| `factum_insert` | 插入新节点 | `node`（S-expression） | — |
| `factum_retract` | 撤回节点（级联） | `node_id` | — |

### 12.3 词素表协商

客户端通过在 `initialize` 请求中声明 `capabilities.factum` 来表明 Factum 感知：

```json
{
  "method": "initialize",
  "params": {
    "capabilities": {
      "factum": {}
    }
  }
}
```

如果客户端声明了 `factum` 能力，服务器在 `InitializeResult` 中返回 `factum_morphemes` 词素表：

```json
{
  "protocolVersion": "2025-06-18",
  "serverInfo": { "name": "factum-mcp", "version": "0.1.0" },
  "factum_morphemes": [
    {"id": 0, "name": "instance-of", "kind": "Relation"},
    {"id": 1, "name": "shareholder-major", "kind": "Relation"},
    ...
  ]
}
```

如果客户端未声明 `factum` 能力（vanilla MCP 客户端），服务器省略词素表，所有词素引用使用字符串名称——**优雅降级**。

### 12.4 资源 URI

节点可通过资源 URI 访问：`factum://nodes/{id}`

### 12.5 扩展合规性

`factum_morphemes` 和 `factum` 能力是 Factum 对 MCP 2025-06-18 规范的**特定扩展**。由于使用标准能力声明模式，严格拒绝未知字段的 MCP 客户端会自动降级到字符串名称形式。

---

## 13. 词素词表系统

### 13.1 什么是词素

词素是 Factum 的内容谓词——如 `shareholder-major`、`instance-of`、`located-in`。每个词素具有：

- 唯一 ID（u32 索引）
- 名称（如 `shareholder-major`）
- 种类（Entity / Relation / Quantifier / Modal / Temporal）
- 类型签名
- 治理状态（Draft / Review / Adopted）

### 13.2 五个种类

| 种类 | 含义 | 示例 |
|------|------|------|
| **Entity** | 实体类型 | organization, person, product, location, event |
| **Relation** | 实体间关系 | instance-of, shareholder-major, subsidiary-of |
| **Quantifier** | 量词 | all, some, most |
| **Modal** | 模态算子 | must, may, should |
| **Temporal** | 时间算子 | since, until, during |

### 13.3 种子词素

v0.1 包含 24 个种子词素：

- **Entity（5）**：organization, person, product, location, event
- **Relation（10）**：instance-of, shareholder-major, subsidiary-of, located-in, founded-on, ceo-of, revenue, employee-count, acquired-by, citizen-of
- **Quantifier（3）**：all, some, most
- **Modal（3）**：must, may, should
- **Temporal（3）**：since, until, during

设计目标：200-500 个词素（从 `morphemes.toml` 通过 `build.rs` 加载）。当前 24 个种子词素足以测试架构，但**不足以用于生产**。

### 13.4 治理生命周期

```rust
pub enum ProposalStatus {
    Draft,    // 已提出，未审查
    Review,   // 审查中
    Adopted,  // 已采纳——稳定，可用于生产节点
}
```

种子词素全部为 `Adopted` 状态。运行时注册的新词素初始为 `Draft`。

### 13.5 线程安全注册表

`MorphemeRegistry` 使用 `parking_lot::RwLock` 保证线程安全：

- `by_name`：`RwLock<AHashMap<SmolStr, MorphemeId>>` ——名称到 ID 的映射
- `by_id`：`RwLock<Vec<Arc<MorphemeDef>>>` ——ID 到定义的映射

注册时使用双重检查锁定模式避免重复注册。

---

## 14. 解析器与 DoS 防护

### 14.1 手写递归下降解析器

Factum 使用手写的递归下降解析器，而非解析器生成器（如 nom、pest）。原因：

- **错误信息控制**：可以生成带有错误分类标识符的精确错误信息
- **零依赖**：不引入解析器生成器依赖
- **深度控制**：所有递归方法通过 `with_depth()` 包装器跟踪嵌套深度

### 14.2 DoS 防护

| 防护 | 值 | 目的 |
|------|---|------|
| `MAX_PARSE_DEPTH` | 128 | 防止深度嵌套输入导致栈溢出 |
| `MAX_TOKENS` | 1,000,000 | 防止超长输入导致 OOM |

128 层深度对合法 Factum-F 足够宽裕（典型节点嵌套 2-3 层），同时远低于大多数平台 8MB 默认栈限制。

### 14.3 错误分类体系

解析错误嵌入错误类标识符，支持 LLM 自我纠正：

| 错误类 | 触发条件 |
|--------|---------|
| `MissingPredField` | 节点缺少 `:pred` 字段 |
| `UnknownNodeField` | 节点包含未知字段关键字 |
| `NamedArgBeforePositional` | 命名参数出现在位置参数之前 |
| `DepthLimitExceeded` | 嵌套深度超过 128 |
| `UnbalancedParen` | 括号不匹配 |
| `UnterminatedString` | 字符串未闭合 |
| `MissingModelRef` | Extracted 溯源缺少模型引用 |

### 14.4 Fuzz 测试

使用 `cargo-fuzz` 进行模糊测试，覆盖三个目标：

- `fuzz_parser`：解析器随机输入
- `fuzz_serialize_roundtrip`：序列化往返一致性
- `fuzz_lexer`：词法器随机输入

---

## 15. 验证器框架

### 15.1 验证器 trait

```rust
pub trait Verifier: Send + Sync {
    fn can_verify(&self, node: &Node) -> bool;
    fn verify(&self, node: &Node) -> Verdict;
}

pub enum Verdict {
    Pass,           // 通过验证
    Fail(String),   // 失败（附原因）
    Inconclusive,   // 无法判定
}
```

### 15.2 内置验证器

| 验证器 | 功能 | 状态 |
|--------|------|------|
| **SchemaVerifier** | 词素签名类型检查（参数数量验证） | ✅ 已实现 |
| **ArithmeticVerifier** | 数值一致性检查（标度 ≤ 38，位数 ≤ 38） | ✅ 已实现 |
| SolverVerifier | Z3 约束满足求解 | 📋 路线图 |
| LeanVerifier | Lean 证明验证 | 📋 路线图 |

### 15.3 验证器注册表

`VerifierRegistry` 按顺序运行所有适用的验证器。只有当所有适用验证器都通过时才返回 `Pass`。任一验证器返回 `Fail` 则立即短路返回。

---

## 16. 查询引擎

### 16.1 查询流程

查询执行遵循严格的 6 步流水线：

1. **候选生成**：通过实体索引或谓词头索引获取候选节点
2. **权限过滤**：在索引层过滤——查询者无权访问的节点不会被物化
3. **有效性过滤**：检查节点在查询时间点是否有效
4. **置信度过滤**：低于阈值的节点被过滤
5. **模式匹配**：将查询模式与候选谓词匹配，提取变量绑定
6. **冲突仲裁**：按指定策略解决匹配同一绑定的多个节点

### 16.2 查询模式

查询使用与节点相同的 S-expression 语法，变量作为通配符：

```scheme
; 查找 ACME-CORP 的大股东，绑定到变量 ?p
(shareholder-major @ACME-CORP ?p)
```

模式匹配支持：
- 变量通配（`?x` 匹配任意值，同一变量多次出现需绑定一致）
- 实体精确匹配（`@ACME-CORP`）
- 字面量精确匹配
- 复合谓词递归匹配
- 列表逐元素匹配

### 16.3 查询选项

```rust
pub struct QueryOptions {
    pub policy: ConflictPolicy,        // 仲裁策略
    pub now: DateTime<Utc>,            // "截至"时间（默认 now）
    pub min_conf: Confidence,          // 最低置信度阈值
    pub perm: PermissionContext,       // 查询者权限上下文
}
```

---

## 17. 项目架构

### 17.1 Crate 结构

```
factum/
├── crates/
│   ├── factum-core/     # 数据模型、词法器、解析器、序列化
│   ├── factum-rt/       # 运行时：存储、查询、仲裁、权限、验证器
│   ├── factum-mcp/      # MCP 桥接：JSON-RPC 工具/资源
│   ├── factum-bench/    # 基准测试：往返、token 效率、查询性能
│   └── factum-demo/     # 端到端演示
├── fuzz/                # cargo-fuzz 目标
├── spec/                # 一致性测试向量（JSON，语言无关）
├── docs/                # 设计文档、编写指南
└── Cargo.toml           # Workspace 根
```

### 17.2 依赖关系

```
factum-mcp → factum-rt → factum-core
factum-bench → factum-rt → factum-core
factum-demo → factum-mcp → factum-rt → factum-core
```

### 17.3 factum-core 依赖

| 依赖 | 用途 |
|------|------|
| serde / serde_json | 序列化 |
| chrono | 时间处理 |
| smol_str | 轻量字符串 |
| thiserror | 错误派生 |
| ahash | 快速哈希 |
| parking_lot | 线程同步 |

**设计原则**：最小依赖核心——不依赖框架、不依赖 tokio、不引入异构依赖。

### 17.4 存储引擎

v0.1 使用内存 `HashMap` 存储，配合 6 个二级索引：

| 索引 | 键 | 值 | 用途 |
|------|---|---|------|
| `by_entity` | EntityId | [NodeId] | 实体查找 |
| `by_pred` | 谓词头名称 | [NodeId] | 谓词查找 |
| `by_src` | 文档/来源 ID | [NodeId] | 文档级撤回 |
| `by_perm` | 权限位掩码 | [NodeId] | 权限过滤 |
| `by_validity` | (from_ts, until_ts) | [NodeId] | 时间范围查询 |
| `deps_rev` | NodeId | [NodeId] | 级联撤回 |

生产版本将使用 RocksDB + WAL + MVCC 替代内存存储。

---

## 18. 路线图

### M0：核心实现 ✅（2026-09-08）

- ✅ factum-core：数据模型、词法器、解析器、序列化
- ✅ factum-rt：内存存储、查询、仲裁、权限、验证器
- ✅ factum-mcp：协议层 + 请求处理器
- ✅ factum-bench：往返、token 效率、查询性能
- ✅ 解析器深度限制 + 词法器 token 限制
- ✅ cargo-fuzz 目标

### M1：开源 Alpha 🚧（目标 2026-09-15）

- ✅ README、SECURITY、CONTRIBUTING、CHANGELOG、CODE_OF_CONDUCT
- ✅ CI：测试 + clippy + fuzz + gitleaks
- ✅ 一致性测试向量
- ✅ 紧凑形式线协议规范
- ✅ 设计理据文档
- ✅ LLM 编写指南
- ✅ Good first issues（9 个预标签）
- ✅ Clippy：0 警告

**退出标准**：fuzzing CI 稳定运行 ≥1 周无崩溃 bug，3+ 外部审查者。

### M2：推广就绪 📋（目标 2026 Q4）

**硬性阻塞项**——任何公开推广前必须完成：

- RocksDB 后端（WAL + MVCC）
- stdio 传输端到端验证
- Streamable HTTP 传输
- 真实 Claude Code / Cursor 集成测试
- Wikidata 转换器 PoC（≥100 万节点）
- 词素表扩展至 200+
- Z3 / Lean 验证器
- **P0**：真实分词器替代启发式估算（issue #9）→ 触发形式定位决策
- 一致性测试向量与 Rust 实现分离
- Python / TypeScript SDK 通过一致性套件

### M3+：研究 📋

- **factum-l**：潜空间投影
  - 编码器 E（Transformer）→ 连续思维向量 z
  - 解码器 D(z) → Factum-F 序列
  - 语义往返目标：≥ 0.95（v0.1），≥ 0.99（验收）
  - SemEquiv 结构化比较（非向量余弦）
  - 训练管线（Qwen2.5-7B-Instruct + LoRA）

### 明确排除项

- **核心 crate 中不引入 GPU 推理**——factum-l 是独立关注点
- **不做 Web 前端**——Inspector 是调试器，不是产品
- **不做云托管**——Factum 是库/协议，不是 SaaS
- **不做付费层**——MIT 许可，永久

---

## 19. 与其他格式的关系

Factum 不是任何现有格式的替代品。它占据特定生态位：为 LLM 读/写/推理设计的结构化知识表示，内置溯源和可验证性。

| 格式 | 定位 | Factum 的差异 |
|------|------|-------------|
| **RDF / JSON-LD** | W3C 语义网标准，三元组 | Factum 节点是七元组，溯源/置信度/有效性/权限为一等公民。RDF 需要 reification 表达溯源。 |
| **CUE** | 配置语言，验证和代码生成 | CUE 验证配置；Factum 验证知识声明，带有时间有效性和冲突仲裁。不同领域。 |
| **Datalog** | 逻辑编程，演绎查询 | Factum 支持模式匹配查询（类 Datalog），但增加时间有效性、置信度加权仲裁和溯源。 |
| **Markdown** | 人类可读文本 | Markdown 面向人类。Factum-F 面向 LLM——以人类可读性换取机器可验证性和无损往返。 |
| **JSON** | 通用数据交换 | JSON 无 schema、无溯源、无时间有效性。Factum 紧凑形式以 JSON 为传输编码但增加结构和类型。 |

**何时使用什么**：
- 需要 SPARQL 端点和 W3C 生态兼容 → RDF/JSON-LD
- 验证应用配置 → CUE
- 规则库上的演绎推理 → Datalog
- 需要 LLM 原生读写结构化知识（带可验证溯源、置信度和时间有效性）→ **Factum**

---

## 20. 风险与缓解

| 模块 | 风险等级 | 缓解措施 |
|------|---------|---------|
| factum-core（解析器、类型） | 低 | Fuzzing、一致性向量、深度限制 |
| factum-rt（存储、查询） | 中 | 仅内存（RocksDB 为路线图）；权限过滤已测试 |
| factum-mcp（桥接） | 中 | 仅协议层；无传输层；未与真实 MCP 主机测试 |
| factum-l（潜空间投影） | 高 | 研究级；技术路线未验证；未开始 |

### 关键风险

1. **Token 效率数据未验证**：当前 token 数据为启发式估算（±15%），真实分词器可能改变结论。缓解：issue #9 追踪，M2 中完成真实测量。

2. **词素词表不足**：24 个种子词素不足以支持生产使用。缓解：M2 扩展至 200+，从 `morphemes.toml` 加载。

3. **MCP 传输未验证**：stdio 传输未与真实 MCP 主机端到端测试。缓解：M2 中完成 Claude Code / Cursor 集成测试。

4. **factum-l 技术路线未验证**：潜空间投影是研究级工作，可能无法达到 ≥ 0.95 语义往返目标。缓解：先实现弱解码器基线，测量后再优化；不阻塞前两层。

---

## 附录 A：完整语法示例

```scheme
; ─── 企业知识图谱示例 ───

; Acme Corp 是一个组织（维基百科断言）
(node n001
  :pred (instance-of @ACME-CORP organization)
  :conf 0.99 :auth 0.95 :perm public
  :src (asserted "wikidata"))

; Acme 总部在深圳
(node n002
  :pred (located-in @ACME-CORP @SHENZHEN)
  :conf 0.95 :auth 0.9 :perm public
  :src (asserted "wikidata"))

; Acme 成立于 1987 年
(node n003
  :pred (founded-on @ACME-CORP #date(1987-04-15))
  :conf 0.99 :auth 0.95 :perm public
  :src (asserted "wikidata"))

; 创始人 1 持有 73% 股份（LLM 从文档提取）
(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :valid (window "2001-03-15T00:00:00+00:00")
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "doc002" [100 200] (model "gpt-4" "2024-06")))

; Acme-Sub 是 Acme-Corp 的子公司（从 n001 派生）
(node n006
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :src (derived n001 "rule-subsidiary-merge")
  :deps [n001])
```

## 附录 B：查询示例

```scheme
; 查找所有组织
(instance-of ?x organization)

; 查找 ACME-CORP 的所有关系（使用实体过滤）
(shareholder-major @ACME-CORP ?holder ?stake)

; 查找位于某地的所有实体
(located-in ?entity @SHENZHEN)
```

## 附录 C：紧凑形式示例

```json
{
  "0": "n004",
  "1": 1,
  "2": ["@ACME-CORP", "@FOUNDER-1", "0.73"],
  "3": [["since", "#date(2001-03-15)"]],
  "4": {"f": "2001-03-15T00:00:00+00:00"},
  "5": {"t": "extracted", "v": {"d": "doc002", "s": [100, 200], "m": ["gpt-4", "2024-06"]}},
  "6": 0.85,
  "7": 0.8,
  "8": 4,
  "9": []
}
```

---

## 附录 D：术语表

| 术语 | 定义 |
|------|------|
| **Node** | Factum 中的基本知识单元，七元组 + deps + status |
| **七元组** | id, predicate, validity, provenance, confidence, authority, permissions |
| **Predicate** | 节点的内容断言，由词素头和参数组成 |
| **Morpheme** | 内容谓词的词汇单元，如 `shareholder-major` |
| **Provenance** | 知识来源的审计追踪，五级分类 |
| **Validity** | 知识的时间有效性窗口 |
| **Dec(i128, u8)** | 无损定点十进制数值表示 |
| **Canonical form** | S-expression 规范形式，用于哈希和往返 |
| **Compact form** | JSON 紧凑形式，用于 MCP 传输 |
| **Arbitration** | 冲突解决策略，Ambiguous 时拒绝猜测 |
| **Cascade retraction** | 源节点撤回时自动撤回派生节点 |
| **SemEquiv** | 结构化语义等价比较（非向量余弦） |

---

*Factum 项目以 MIT 许可证开源。"Factum" 和 Factum-F 语言规范为项目名称，MIT 许可证仅覆盖代码；规范未来可能由独立流程管理。*

*贡献需要 DCO 签名（`git commit -s`）。参见 CONTRIBUTING.md。*
