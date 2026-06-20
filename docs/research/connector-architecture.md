# Connector Architecture: Survey of AI Agent Frameworks

Research date: 2026-06-20. Covers elizaOS, Goose, OpenHands, Letta/MemGPT, Rig, LangGraph, agent-protocol, and Telegram/Discord bot patterns.

---

## 1. The Common Connector Seam — Names Used Across Frameworks

| Framework | Term used | Notes |
|-----------|-----------|-------|
| **elizaOS** | **Service** (v2) / **Client** (v1) | Discord, Telegram, Twitter each register as a `Service` with `ServiceType::DISCORD` etc. v1 used `Client[]` inside a `Plugin`. |
| **Goose (Block)** | **Extension** (via MCP) | Goose is MCP-native; every integration is an MCP extension server. No proprietary connector concept. |
| **OpenHands** | No named abstraction — uses `LocalConversation` / `RemoteConversation` factory | The `Conversation()` factory dispatches to local execution or HTTP+WebSocket server transparently. |
| **Letta/MemGPT** | No connector layer — **REST API is the seam** | All clients (web, SDK, Slack bots) call `POST /agents/{id}/messages` directly; the server is the single point. |
| **Rig (Rust)** | No connector layer — **`Agent<M>` is a library type, not a server** | Rig gives you a typed agent; you host it inside any Axum/Actix server or CLI you build. |
| **LangGraph** | **Client / RunsClient** | `LangGraphClient` wraps all server calls. Server-side: Assistants, Threads, Runs, Store are the four resources. |
| **agent-protocol (langchain-ai)** | **Run / Thread** as protocol primitives | Stateless "Run" + stateful "Thread" + "Store" is the lingua franca; connectors are not named. |
| **AutoGPT (old agent-protocol.ai)** | **Task / Step / Artifact** | Older REST spec; now largely superseded by LangGraph's threads/runs shape. |

### Recommendation for OxideMX

Use the term **Connector**. It is:
- Not overloaded by any framework (Service/Client/Extension/Channel are all taken in ways that conflict with our Rust idioms).
- Matches the mental model: something that "connects" an external channel to the agent core.
- Composes naturally: `OverlayConnector`, `TelegramConnector`, `HttpConnector`, `DiscordConnector`.

---

## 2. The Seam Shape — What a Connector Trait Looks Like

### Common pattern across all frameworks

Every framework that has a real connector abstraction resolves to the same three responsibilities:

1. **Inbound**: receive channel-specific events, normalize to a shared `Message` type, hand to the agent core.
2. **Outbound**: accept agent reply (text, streaming tokens, structured data), format for the channel, deliver.
3. **Lifecycle**: start (connect, authenticate), stop (disconnect, flush).

### elizaOS Service (most explicit real interface found)

```typescript
abstract class Service {
  static serviceType: ServiceType;       // e.g. ServiceType.DISCORD
  abstract capabilityDescription: string;
  static async start(runtime: IAgentRuntime): Promise<Service>;
  abstract stop(): Promise<void>;
}
```

The Discord service binds `client.on('messageCreate', async (msg) => { ... })` to convert the Discord message into an elizaOS `Memory`, then calls `runtime.processActions(memory, [])`. The Telegram service does the same via the Telegram Bot API long-poll.

### Recommended Rust trait for OxideMX

```rust
#[async_trait]
pub trait Connector: Send + Sync + 'static {
    /// Human-readable name, used in logs and routing ("overlay", "telegram", "http").
    fn name(&self) -> &str;

    /// Called once at daemon start. Spawns whatever polling/server loop the
    /// channel needs and sends normalized InboundEvent values to the agent core.
    async fn start(&self, tx: mpsc::Sender<InboundEvent>) -> anyhow::Result<()>;

    /// Called by the agent core to deliver a reply back to this channel.
    async fn send(&self, reply: OutboundMessage) -> anyhow::Result<()>;

    /// Graceful shutdown (cancel polls, close sockets, drain sends).
    async fn stop(&self) -> anyhow::Result<()>;
}
```

The `agentd` main loop becomes:

```rust
// Each connector runs its own task, all share a single inbound channel.
let (tx, mut rx) = mpsc::channel::<InboundEvent>(256);
for connector in connectors {
    let tx2 = tx.clone();
    tokio::spawn(async move { connector.start(tx2).await });
}
while let Some(event) = rx.recv().await {
    let reply = agent_core.handle(event).await?;
    connectors[reply.connector_id].send(reply).await?;
}
```

### Session/conversation identity across channels

| Framework | Identity primitive |
|-----------|--------------------|
| elizaOS | `roomId` (UUID) — shared across all platforms for the same logical conversation |
| LangGraph | `thread_id` (UUID) + `run_id` per invocation |
| Letta | `agent_id` + optional `conversation_id` (conversations are separate from agents) |
| OpenHands | `conversation_id` via `POST /conversations` |
| Telegram bots | `chat_id` (long integer, unique per user+bot pair) |
| Discord bots | `channel_id` + optional `thread_id` for forum threads |

**Recommendation**: use `session_id: Uuid` in `InboundEvent`. Each Connector is responsible for mapping its channel's native identity (chat_id, roomId, thread_id) to a session_id, and for routing `OutboundMessage { session_id, ... }` back to the correct channel handle.

---

## 3. Normalized Message / Event Model

### elizaOS Memory (most complete normalized model found)

```typescript
interface Memory {
  id?: UUID;
  entityId: UUID;           // sender (user or agent)
  agentId?: UUID;
  roomId: UUID;             // conversation / channel identity
  worldId?: UUID;           // optional: server/workspace
  content: Content;
  embedding?: number[];     // for semantic search
  createdAt?: number;       // ms since epoch
  metadata?: MemoryMetadata;
}

interface Content {
  text?: string;
  source?: string;          // "discord", "telegram", "http"
  url?: string;
  attachments?: Attachment[];
  actions?: string[];       // intent hints
  [key: string]: any;
}
```

elizaOS Socket.IO broadcast payload (internal messaging bus):
```json
{
  "roomId": "uuid",
  "channelId": "uuid",
  "senderName": "Alice",
  "text": "hello",
  "metadata": {}
}
```

### OpenHands WebSocket event (actual field found in docs)
```json
{ "type": "message", "content": "..." }
```

### Recommended OxideMX model

```rust
/// Normalized inbound event from any connector.
pub struct InboundEvent {
    pub id: Uuid,
    pub session_id: Uuid,          // maps to connector's native chat/room/thread
    pub connector: String,         // "overlay", "telegram", "http"
    pub sender: SenderInfo,
    pub content: EventContent,
    pub timestamp: DateTime<Utc>,
    pub capabilities: ConnectorCaps, // can_stream, can_attach_files, can_react
}

pub struct SenderInfo {
    pub id: String,                // platform-specific user id
    pub display_name: String,
    pub is_bot: bool,
}

pub enum EventContent {
    Text(String),
    VoiceNote { url: String, duration_secs: u32 },
    File { url: String, mime: String, name: String },
    Command { name: String, args: Vec<String> },  // slash commands
    Reaction { emoji: String, target_message_id: Uuid },
}

pub struct ConnectorCaps {
    pub can_stream: bool,          // overlay: yes; Telegram: no
    pub can_edit_message: bool,    // Discord/Telegram: yes
    pub can_thread: bool,
    pub max_message_len: usize,
}

/// Outbound reply from agent core to a connector.
pub struct OutboundMessage {
    pub session_id: Uuid,
    pub reply_to_id: Option<Uuid>,  // optional in-reply-to
    pub content: ReplyContent,
}

pub enum ReplyContent {
    Text(String),
    Stream(Pin<Box<dyn Stream<Item = String> + Send>>),
    ToolApprovalRequest(ApprovalRequest),
    StatusUpdate(String),          // "thinking...", progress bars
}
```

---

## 4. HTTP API Connector — Threads/Runs/Messages/SSE Shape

### LangGraph Server API (most complete standardized shape)

This is the de-facto standard in 2024-2026, also published by langchain-ai as a standalone `agent-protocol` spec that "LangGraph Platform implements a superset of."

**Four resource types: Assistants, Threads, Runs, Store**

```
# Assistants — named agent configurations
POST   /assistants
GET    /assistants/{assistant_id}
PATCH  /assistants/{assistant_id}

# Threads — persistent conversation containers (hold state/history)
POST   /threads                              → { thread_id, ... }
GET    /threads/{thread_id}
GET    /threads/{thread_id}/state            → current graph state
GET    /threads/{thread_id}/history          → all checkpoints

# Runs — single invocations within a thread
POST   /threads/{thread_id}/runs             → background run
POST   /threads/{thread_id}/runs/stream      → SSE stream
POST   /threads/{thread_id}/runs/wait        → blocking, returns final state
GET    /threads/{thread_id}/runs/{run_id}
POST   /threads/{thread_id}/runs/{run_id}/cancel

# Stateless runs (no thread)
POST   /runs/stream
POST   /runs/wait

# Cross-thread store (long-term memory)
PUT    /store/items
GET    /store/items
POST   /store/items/search
```

**SSE event format** (streaming run):
```
event: metadata
data: {"run_id":"<uuid>","graph_id":"<name>"}

event: values
id: 0
data: {"messages":[...],"other_state_key":"..."}

event: updates
id: 1
data: {"messages":[{"role":"assistant","content":"Hello"}]}

event: messages
id: 2
data: [{"type":"AIMessageChunk","content":"Hel"},...]
```

Multiple `stream_mode` values can be requested: `values`, `updates`, `messages`, `events`, `tasks`, `checkpoints`. `Last-Event-ID` header enables reconnect/resume.

**Run object fields**: `run_id`, `thread_id`, `assistant_id`, `status` (pending/running/interrupted/succeeded/failed), `input`, `created_at`, `checkpoint_id`.

### Letta API (simpler, agent-centric)

```
POST   /agents/{agent_id}/messages           → create + get reply
POST   /agents/messages/stream               → SSE stream
POST   /conversations                        → persistent dialogue container
POST   /conversations/{id}/messages
POST   /conversations/messages/stream
```

Messages keyed by `agent_id`; conversations are optional multi-turn wrappers over the same agent.

### Old agent-protocol.ai (AutoGPT era — largely historical)

```
POST   /ap/v1/agent/tasks                    → create task
GET    /ap/v1/agent/tasks/{task_id}/steps
POST   /ap/v1/agent/tasks/{task_id}/steps    → advance one step
POST   /ap/v1/agent/tasks/{task_id}/artifacts
```

Task/Step/Artifact model. Not streaming-native. Largely superseded.

### Recommended OxideMX HTTP Connector API

Follow the LangGraph threads/runs shape since that is the converging standard and it is what Claude Code's API-driving capabilities will expect. Minimum viable:

```
POST   /conversations                          → { session_id }
POST   /conversations/{session_id}/messages    → { message_id, reply: string }
POST   /conversations/{session_id}/messages/stream → SSE stream
GET    /conversations/{session_id}/messages    → history
GET    /health
```

SSE event types: `token` (streaming chunk), `done` (stream complete), `error`, `tool_call`, `approval_request`.

---

## 5. Recommendation for OxideMX Design

### How D-Bus/overlay becomes one Connector among many

Today, agentd speaks D-Bus only and the overlay is the sole client. The refactor:

1. Define the `Connector` trait above.
2. Move all D-Bus logic into `OverlayConnector`, which:
   - Listens on the existing D-Bus `send_message(text)` signal → produces `InboundEvent`.
   - Exposes the existing D-Bus `event(payload)` signal for `OutboundMessage` delivery.
   - Sets `can_stream: true` (overlay renders streaming tokens via the existing chat bubble UI).
3. Add `TelegramConnector` (long-poll via `teloxide` crate, maps `chat_id → session_id`).
4. Add `HttpConnector` (Axum server, threads/runs REST shape, SSE for streaming).
5. Add `DiscordConnector` as needed.

### Mapping to existing agentd pieces

| Current agentd concept | Maps to Connector model |
|------------------------|------------------------|
| D-Bus `send_message(text)` | `InboundEvent { connector: "overlay", content: Text(...) }` |
| D-Bus `event` signal | `OutboundMessage` → `OverlayConnector::send()` |
| Agent session (single-user overlay) | `session_id` = fixed UUID for the overlay session |
| `agent_runtime.rs` agent loop | Becomes the central `while let Some(event) = rx.recv()` loop |
| Tool approval flow | `ReplyContent::ToolApprovalRequest` → connector-specific rendering |

### Connector registry in agentd

```rust
pub struct ConnectorRegistry {
    connectors: HashMap<String, Arc<dyn Connector>>,
}

impl ConnectorRegistry {
    pub async fn start_all(&self, tx: mpsc::Sender<InboundEvent>) { ... }
    pub async fn send(&self, msg: OutboundMessage) -> anyhow::Result<()> {
        self.connectors[&msg.session_id_to_connector(...)].send(msg).await
    }
}
```

Session→connector mapping can be a `HashMap<Uuid, String>` maintained as connectors create new sessions.

### Streaming vs non-streaming connectors

- **Overlay** (`can_stream: true`): agent core streams tokens via `ReplyContent::Stream`; `OverlayConnector` forwards each chunk over D-Bus `event` signal as a `{type: "token", text: "..."}` payload.
- **Telegram** (`can_stream: false`): `TelegramConnector::send()` receives the full `Text` reply after the agent finishes. Intermediate status can be delivered via Telegram's `sendChatAction("typing")`. If the agent reply is structured as a `Stream`, the connector buffers it to completion before calling `bot.send_message()`.
- **HTTP SSE** (`can_stream: true`): `HttpConnector` writes each `token` event to the SSE sink; `done` event closes the stream. Non-streaming HTTP callers use the blocking `POST /messages` variant which awaits completion internally.

The `ConnectorCaps::can_stream` field lets the agent core decide whether to emit `ReplyContent::Stream` or buffer to `ReplyContent::Text` before handing off.

---

## Sources

- [ElizaOS Documentation](https://docs.elizaos.ai/)
- [ElizaOS Plugin Reference](https://docs.elizaos.ai/plugins/reference.md)
- [ElizaOS Messaging](https://docs.elizaos.ai/runtime/messaging.md)
- [ElizaOS Services](https://docs.elizaos.ai/runtime/services.md)
- [ElizaOS Discord Event Flow](https://docs.elizaos.ai/plugin-registry/platform/discord/event-flow.md)
- [Eliza paper (arxiv 2501.06781)](https://arxiv.org/html/2501.06781v1)
- [LangGraph API Endpoints (DeepWiki)](https://deepwiki.com/langchain-ai/langgraphjs/5.3-api-endpoints-and-resources)
- [LangGraph Streaming Changelog](https://changelog.langchain.com/announcements/reliable-streaming-and-efficient-state-management-in-langgraph)
- [LangGraph Threads/State (DeepWiki)](https://deepwiki.com/langchain-ai/langgraph/7.2-threads-and-state-management)
- [langchain-ai/agent-protocol (GitHub)](https://github.com/langchain-ai/agent-protocol)
- [Letta Core Concepts](https://docs.letta.com/core-concepts/)
- [Letta API Reference](https://docs.letta.com/api-reference/overview/)
- [OpenHands Agent Server Docs](https://docs.openhands.dev/sdk/arch/agent-server)
- [Rig Rust framework](https://rig.rs/)
- [Rust-Native AI Agent Frameworks 2026 (Zylos)](https://zylos.ai/research/2026-04-01-rust-native-ai-agent-frameworks-ecosystem-2026/)
- [Goose by Block](https://block.xyz/inside/block-open-source-introduces-codename-goose)
- [OpenClaw Channel Adapter pattern](https://docs.openclaw.ai/channels/telegram)
- [Hermes Agent Telegram](https://hermes-agent.nousresearch.com/docs/user-guide/messaging/telegram/)
