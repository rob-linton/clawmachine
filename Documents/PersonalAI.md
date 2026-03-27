# Personal AI — Infinite Memory Architecture

## What It Is

Every user of Claw Machine gets a **persistent AI colleague** — an assistant that never forgets anything you've ever told it. It learns your preferences, tracks your projects, remembers your team, and develops genuine understanding of how you work. Delete your chat, restart the server, come back in a month — it still knows you.

This isn't memory bolted onto a chatbot. It's a system where Claude maintains its own notebook, thinks between conversations, and uses the full power of the workspace as an extension of its mind.

## How It Feels to the User

### First Conversation
You chat normally. Behind the scenes, Claude starts building a picture of who you are — your role, what you're working on, how you communicate. By the end of the first session, its notebook has entries about you.

### Second Conversation
When you return, Claude greets you with context:

> *Previously: You've been building the inventory API for the React dashboard. Last session, you implemented the /products endpoint with cursor-based pagination. You mentioned a demo for stakeholders on April 3rd — that's 5 days away. Alice was going to review PR #42.*

It picks up exactly where you left off. No re-explaining. No "as a new conversation, I don't have context."

### Tenth Conversation
Claude knows your coding style, your team, your architectural preferences. It remembers that you chose JWT over sessions three weeks ago and why. When you mention "the auth bug," it knows which one. When you ask for code, it writes it the way you like it.

### After a Break
You come back after two weeks. Claude's consolidation process has refined its notes. It has a clear, concise understanding of your projects, your team, and your preferences. It doesn't remember every trivial detail — but it remembers everything that matters.

---

## Architecture Overview

### The Three Tiers

```
┌─────────────────────────────────────────────────────────┐
│  Tier 1: HOT — CLAUDE.md (rewritten every message)     │
│                                                         │
│  What Claude "just knows" — injected into system prompt │
│  before every message. No file reads needed.            │
│                                                         │
│  Contains:                                              │
│  • Temporal context (today's date, session timing)      │
│  • User profile (from notebook/about-user.md)           │
│  • Active projects (from notebook/active-projects.md)   │
│  • Top 15 memories ranked by importance                 │
│  • Anticipation note ("user might need X next")         │
│  • Notebook usage instructions                          │
│                                                         │
│  • Session index (recent conversation digests by date)   │
│                                                         │
│  Budget: ~2000 tokens. Refreshed before every message.  │
├─────────────────────────────────────────────────────────┤
│  Tier 2: WARM — .notebook/ workspace files              │
│                                                         │
│  What Claude can look up — files in the workspace that  │
│  Claude reads when it needs detail.                     │
│                                                         │
│  Structure:                                             │
│  .notebook/                                             │
│  ├── about-user.md        — role, expertise, style      │
│  ├── active-projects.md   — what they're building       │
│  ├── decisions.md         — decision log with rationale │
│  ├── people.md            — team members and roles      │
│  ├── preferences.md       — tools, frameworks, style    │
│  ├── timeline.md          — key events with dates       │
│  ├── topics/              — deep notes per topic        │
│  │   ├── authentication.md                              │
│  │   └── api-design.md                                  │
│  ├── sessions/            — conversation digests        │
│  │   ├── 2026-03-25-auth-middleware.md                   │
│  │   └── 2026-03-22-inventory-api.md                    │
│  └── scratch.md           — working notes (this session)│
│                                                         │
│  Persisted to Redis per-user. Survives everything.      │
│  Claude reads AND writes to these naturally.            │
├─────────────────────────────────────────────────────────┤
│  Tier 3: COLD — Redis archive                           │
│                                                         │
│  Full message history with per-message summaries.       │
│  Not in the workspace — retrieved on demand.            │
│  Used for: rehydration, rolling summaries, search.      │
│                                                         │
│  Also on disk: .chat/messages/{seq}-{role}.md           │
│  Searchable via grep from within Claude's session.      │
└─────────────────────────────────────────────────────────┘
```

### The Notebook

The notebook is what makes this system feel human. Instead of a flat database of "facts," Claude maintains structured notes — the same way a real colleague would.

| File | Purpose | Updated By |
|------|---------|------------|
| `about-user.md` | Who the user is, their expertise, communication style | Cognitive pipeline + Claude |
| `active-projects.md` | Current work, status, blockers | Cognitive pipeline + Claude |
| `decisions.md` | Append-only log: date, decision, rationale, alternatives | Cognitive pipeline + Claude |
| `people.md` | Team members, roles, relationships | Cognitive pipeline + Claude |
| `preferences.md` | Tools, frameworks, response style preferences | Cognitive pipeline + Claude |
| `timeline.md` | Key events and deadlines with absolute dates | Cognitive pipeline + Claude |
| `topics/{name}.md` | Deep notes on specific subjects discussed at length | Claude + consolidation |
| `sessions/{date}-{slug}.md` | Conversation digests — narrative recaps of past sessions | Session digest pipeline |
| `scratch.md` | Working notes for the current session | Claude (cleared on consolidation) |

The notebook is **bidirectional**:
- **Claude writes to it** during conversations (explicit memory — "I should remember this")
- **The cognitive pipeline writes to it** after conversations (implicit extraction — patterns Claude didn't consciously note)
- **Consolidation refines it** during idle time (synthesis — merging, pruning, elevating)

### The Flows

```
═══════════════════════════════════════════════════════════
                    BEFORE EACH MESSAGE
═══════════════════════════════════════════════════════════

  Redis ──deploy──→ .notebook/ files in workspace
                    (restore user's full notebook)

  Redis ──build───→ CLAUDE.md
                    (dynamic: temporal context + user
                     profile + top memories + anticipation)

  If container restarted:
  Redis ──build───→ "Previously On..." narrative
                    (prepended to user's message)

═══════════════════════════════════════════════════════════
                    DURING THE MESSAGE
═══════════════════════════════════════════════════════════

  Claude reads CLAUDE.md     → knows who the user is
  Claude reads .notebook/    → can look up details
  Claude writes .notebook/   → explicitly remembers things
  Claude reads .chat/messages/ → can search full history

═══════════════════════════════════════════════════════════
                    AFTER EACH MESSAGE
═══════════════════════════════════════════════════════════

  1. Harvest: .notebook/ changes ──→ Redis
     (Claude's explicit memory writes, persisted immediately)

  2. Cognitive Pipeline (background, in summarizer container):
     Stage 1+2: Extract & Connect
       → One-line summary of the exchange
       → Notebook updates (new facts, updated entries)
     Stage 3: Reflect
       → Mood assessment (productive/debugging/frustrated/...)
     Stage 4: Anticipate (every 5th message)
       → "User might need X next" (injected into next CLAUDE.md)

  3. Rolling Summary (every 10th message):
     → Rebuild .chat/summary.md from all message summaries
     → Used for rehydration on container restart

═══════════════════════════════════════════════════════════
                    ON IDLE (between sessions)
═══════════════════════════════════════════════════════════

  Consolidation Pass:
  → Merge scratch.md notes into topic files
  → Update about-user.md with fresh synthesis
  → Update active-projects.md with current status
  → Archive low-importance entries
  → Add session summary to timeline.md

  Session Digest:
  → Generate narrative recap of exchanges since last digest
  → Store as .notebook/sessions/{date}-{topic-slug}.md
  → Covers: topics discussed, decisions made, code built,
    problems solved, important context for later recall
  → Tracked via last_digest_seq in NotebookMeta

  This is the "sleeping on it" step — the agent processes
  the session and emerges with cleaner understanding.

═══════════════════════════════════════════════════════════
```

### Temporal Awareness

Every CLAUDE.md injection includes time context:

```markdown
## Temporal Context
- Today: Thursday, March 27, 2026, 3:45 PM
- This session started: 45 minutes ago (12 messages)
- Last session: yesterday at 4:00 PM (worked on auth endpoint)
- Upcoming: Demo for stakeholders — April 3 (7 days away)
- Working on inventory API for 3 days
```

This enables Claude to:
- Say "you mentioned this yesterday" instead of "in a previous message"
- Proactively flag approaching deadlines
- Understand the rhythm of the user's work

### Container Restart Recovery

When the Docker session container dies (idle timeout, crash, server restart) and a new one is created:

1. **Detection**: `ensure_container()` returns `is_new=true`. If `seq > 1`, rehydration is needed.
2. **Notebook restored**: All `.notebook/` files deployed from Redis (user never loses memories).
3. **CLAUDE.md rebuilt**: Fresh dynamic CLAUDE.md with full context.
4. **Narrative preamble**: The user's message is wrapped with a "Previously On..." recap built from the rolling summary + recent messages + notebook highlights.
5. **No `--continue`**: Since there's no conversation to continue, the first message in the new container runs without `--continue`. Subsequent messages use `--continue` normally.

The user experiences zero disruption. Claude responds as if nothing happened.

### Conversation Recall

Instead of managing multiple chat windows, the Personal AI uses a single continuous conversation with deep recall. When the user says something like "remember that conversation about auth from last week," Claude:

1. **Checks the session index** in CLAUDE.md — sees `2026-03-20-auth-middleware.md` listed
2. **Reads the digest** at `.notebook/sessions/2026-03-20-auth-middleware.md` — gets a 200-400 word narrative recap covering topics, decisions, code built, and open problems
3. **Optionally searches `.chat/messages/`** for the actual exchange if still in history
4. **Responds with full context** as if it remembers the conversation naturally

Session digests are generated during the consolidation pass (idle timeout) and stored in the notebook — they survive chat deletion, container restarts, and server reboots. The CLAUDE.md hot tier includes a session index (date + topic slug + first line) for the 20 most recent digests, so Claude always knows what's available to recall without reading every file.

This approach avoids the cognitive overhead of switching between conversations. The user has one relationship with one assistant. Past conversations are recalled naturally through language, not through UI navigation.

### Chat Deletion & Recreation

When a user deletes their chat and creates a new one:

1. Chat messages are deleted (cold tier gone)
2. Notebook entries in Redis are **preserved** (they're per-user, not per-session)
3. New chat workspace gets `.notebook/` restored from Redis
4. New CLAUDE.md is generated with full user context
5. Claude starts the new chat already knowing who the user is

The only thing lost is the raw message history. Everything Claude *learned* from those messages survives.

---

## Cognitive Pipeline Detail

The cognitive pipeline runs in a **dedicated long-running summarizer container** — separate from the chat session container. This avoids the `--continue` poisoning bug that killed the original summarizer.

```
Chat Container (claw-chat-{id})          Summarizer Container (claw-summarizer-{pid})
┌──────────────────────┐                 ┌──────────────────────┐
│ /workspace            │                 │ /tmp/summarizer/      │
│ CLAUDE.md             │                 │   {uuid1}/            │
│ .notebook/            │                 │   {uuid2}/            │
│ .chat/messages/       │                 │   {uuid3}/            │
│                       │                 │                       │
│ claude -p --continue  │                 │ claude -p (no continue)│
│ (stateful session)    │                 │ (stateless, isolated)  │
└──────────────────────┘                 └──────────────────────┘
```

Each cognitive pipeline call uses a **unique subdirectory** inside the summarizer container (`/tmp/summarizer/{uuid}/`), so even if multiple chats run pipelines concurrently, their Claude sessions never interfere.

### Pipeline Stages

| Stage | Purpose | Model | Frequency |
|-------|---------|-------|-----------|
| 1+2. Extract & Connect | Summary + notebook operations | Haiku | Every message |
| 3. Reflect | Mood assessment | Haiku | Every message |
| 4. Anticipate | Predict what user needs next | Haiku | Every 5th message |
| Consolidation | Merge, prune, synthesize | Sonnet | On idle timeout |
| Session Digest | Narrative recap of recent exchanges | Sonnet | On idle timeout (after consolidation) |
| Rolling Summary | Rebuild .chat/summary.md | Haiku | Every 10th message |

### Memory Importance Scoring

When selecting which memories go into CLAUDE.md (hot tier), entries are scored:

```
score = recency_weight × frequency_weight × type_weight

recency_weight  = 1.0 / (1 + days_since_last_access)
frequency_weight = log(1 + access_count)
type_weight     = { decision: 3.0, project: 2.5, user: 2.0, preference: 1.5, fact: 1.0 }
```

Top 15 entries by score are included in CLAUDE.md. The rest remain accessible in `.notebook/` files.

---

## Redis Schema

```
# Per-user notebook (survives everything)
claw:user:{username}:notebook              — Set of file paths
claw:user:{username}:notebook:{path}       — JSON NotebookEntry
claw:user:{username}:notebook_meta         — JSON: {total_entries, last_consolidation, last_digest_seq, mood_history, anticipation}

# Per-chat (tied to session lifecycle)
claw:chat:{chat_id}                        — JSON ChatSession
claw:chat:{chat_id}:messages               — Sorted set of messages (with summary field)
claw:chat:{chat_id}:container              — Session container name
claw:chat:{chat_id}:seq_counter            — Atomic message counter
claw:chat:{chat_id}:exec_lock              — Per-chat execution lock
claw:chat:{chat_id}:stream                 — Pub/sub for streaming
```

---

## What Makes This Different

| Feature | Typical AI Memory | Claw Machine Personal AI |
|---------|-------------------|--------------------------|
| Storage | Flat key-value facts | Structured notebook (decisions, people, projects, timeline) |
| Context injection | Static system prompt | Dynamic CLAUDE.md rewritten every message |
| Time awareness | None | Full temporal context (dates, deadlines, session timing) |
| Memory source | Extraction only | Bidirectional: Claude writes + system extracts + consolidation refines |
| Between messages | Nothing | Consolidation pass: merge, prune, synthesize |
| Container restart | Context lost | "Previously On..." narrative with zero disruption |
| Session deletion | Memory lost | Notebook survives (per-user, not per-session) |
| Mood awareness | None | Tracks productive/debugging/frustrated/exploring |
| Anticipation | None | Predicts what user needs next, injects into CLAUDE.md |
| Memory ranking | All equal | Importance-scored: recency × frequency × type weight |
| Past conversations | Start new chat, lose context | Single chat with natural recall ("remember when we discussed...") |
