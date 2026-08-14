# Getting Started

This walks through a first conversation, from nothing to three models arguing
with each other. Fifteen minutes, no prior knowledge assumed.

## 1. Get a model to talk to

The quickest start is [Ollama](https://ollama.com), which runs models on your
own machine and needs no account:

```bash
ollama pull llama3:8b
ollama pull gemma3
```

Two different models make a much better room than one model twice, because they
actually disagree. Any two will do.

## 2. Build and start

```bash
git clone https://github.com/9t29zhmwdh-coder/HiveMind_Chat.git
cd HiveMind_Chat

cp hivemind.example.toml hivemind.toml
(cd frontend && npm ci && npm run build)
cargo build --release

./target/release/hivemind-server
```

The first build takes a few minutes. When it starts you should see:

```
INFO hivemind_server: no credential needed id=local url=http://127.0.0.1:11434
INFO hivemind_server: HiveMind Chat is listening bind=127.0.0.1:8750
```

The first line is the check that matters: it means the server found your local
endpoint. Open <http://127.0.0.1:8750>.

## 3. Build a room

Click **+** next to *Rooms*.

- **Name:** anything.
- **Policy:** start with **debate**. It hands out opposing stances, which makes
  the difference between the models obvious immediately.
- **Rounds:** 1 to start. Raise it later; each round is another full pass
  through every agent.

Then **Add agent**, twice:

| Field | First agent | Second agent |
|---|---|---|
| Name | Scout | Vera |
| Provider | Ollama | Ollama |
| Model | llama3:8b | gemma3 |
| Persona | You favour simplicity and few moving parts. Answer in under 60 words. | You favour operational maturity and scale. Answer in under 60 words. |

The persona is the single most useful field. It is what turns two
interchangeable assistants into two participants with a position. The word
limit matters too: without it, local models tend to write essays.

Save, and ask something with a real trade-off in it:

> We are building a homelab tool that stores a few thousand rows. SQLite or Postgres?

## 4. What you should see

Scout answers first, then Vera answers **Scout by name** and argues the other
side. That is the debate policy working: each agent sees the other's turn and
gets an assigned stance.

If both agents just agree politely, the personas are too similar. Push them
further apart.

## 5. Try the other policies

Open the room's settings and change the policy:

- **parallel** asks both models the same question in isolation. Neither sees
  the other's answer, so this is the honest side-by-side comparison. Use it to
  decide which model to trust for a kind of question.
- **consensus** runs the discussion and then makes everyone vote. The votes
  appear as a separate block at the end, not as more chat.
- **moderated** needs three agents: one moderator plus two others. The
  moderator decides who speaks each round.

## 6. The terminal, if you prefer it

Everything above works without a browser, against the same database:

```bash
hive providers
hive rooms
hive chat <room-id> "Which one would you actually ship?"
hive export <room-id> > transcript.md
```

## 7. Adding a hosted model

Only if you want one. Add the account to `hivemind.toml`:

```toml
[[providers]]
id = "anthropic-main"
label = "Anthropic"
kind = "anthropic"
api_key_env = "HIVEMIND_KEY_ANTHROPIC"
```

Note what that says: the **name** of an environment variable, not the key. Set
the value in your shell and restart the server:

```bash
export HIVEMIND_KEY_ANTHROPIC="..."
./target/release/hivemind-server
```

`hive providers` now shows whether the key resolves. A hosted model in a room
with local ones is where this tool gets interesting: you can watch a large
model and a small one disagree, and decide whether the difference is worth the
money for your kind of question.

## Troubleshooting

**"connection refused" from the local provider.** Ollama is not running. Start
it and reload.

**An agent reports "the model returned no text".** Usually the model name is
wrong. `hive models local` prints what your endpoint actually serves, and the
model field in the agent dialog offers the same list.

**A hosted agent fails with HTTP 401.** The credential did not resolve. Check
that the variable named in `api_key_env` is exported in the shell that started
the server, then check `hive providers`.

**Answers get cut off mid-sentence.** Raise the agent's max tokens. If the
agent has reasoning switched on, raise it further: reasoning is drawn from the
same budget as the answer.
