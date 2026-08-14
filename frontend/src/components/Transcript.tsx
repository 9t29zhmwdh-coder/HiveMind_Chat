import { useEffect, useMemo, useRef } from "react";
import type { Strings } from "../i18n";
import type { Agent, Message, PendingTurn, Vote } from "../types";

interface Props {
  strings: Strings;
  messages: Message[];
  pending: PendingTurn[];
  votes: Vote[];
  agents: Agent[];
  round: { current: number; total: number } | null;
  /** Lay peer answers out side by side instead of one after another. */
  compare: boolean;
  query: string;
}

/** A single turn, or a set of turns that answered the same prompt. */
type Block = { kind: "single"; message: Message } | { kind: "compare"; messages: Message[] };

export function Transcript({
  strings,
  messages,
  pending,
  votes,
  agents,
  round,
  compare,
  query,
}: Props): React.ReactElement {
  const endRef = useRef<HTMLDivElement>(null);
  const streamedLength = pending.reduce((total, turn) => total + turn.text.length, 0);

  const visible = useMemo(() => filterMessages(messages, query), [messages, query]);
  const blocks = useMemo(() => groupTurns(visible, compare), [visible, compare]);

  // Follow the conversation as it grows, including while a turn streams.
  // Searching is the exception: jumping to the end would fight the reader.
  useEffect(() => {
    if (!query) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [messages.length, streamedLength, votes.length, query]);

  const colourOf = (agentId: string | null): string =>
    agents.find((agent) => agent.id === agentId)?.colour ?? "#6c6c78";

  const turn = (message: Message): React.ReactElement => (
    <article
      key={message.id}
      className={message.agent_id ? "turn" : "turn user"}
      style={message.agent_id ? { borderLeftColor: colourOf(message.agent_id) } : undefined}
    >
      <header>
        <span className="who" style={{ color: message.agent_id ? colourOf(message.agent_id) : undefined }}>
          {message.agent_id ? message.speaker : strings.you}
        </span>
        <span className="when">{formatTime(message.created_at)}</span>
      </header>
      <div className="body">{highlight(message.content, query)}</div>
    </article>
  );

  return (
    <div className="transcript">
      {query && (
        <div className="round-divider">
          {visible.length} {strings.searchResults}
        </div>
      )}

      {blocks.map((block, index) =>
        block.kind === "single" ? (
          turn(block.message)
        ) : (
          <div className="compare-grid" key={`compare-${index}`}>
            {block.messages.map(turn)}
          </div>
        ),
      )}

      {round && !query && (
        <div className="round-divider">
          {strings.round} {round.current}/{round.total}
        </div>
      )}

      {!query && (
        <div className={compare && pending.length > 1 ? "compare-grid" : undefined}>
          {pending.map((streaming) => (
            <article key={streaming.agentId} className="turn streaming" style={{ borderLeftColor: streaming.colour }}>
              <header>
                <span className="who" style={{ color: streaming.colour }}>
                  {streaming.agentName}
                </span>
              </header>
              <div className="body">{streaming.text}</div>
            </article>
          ))}
        </div>
      )}

      {votes.length > 0 && !query && (
        <section className="votes">
          <h3>{strings.votes}</h3>
          <ul>
            {votes.map((vote) => (
              <li key={vote.agentId}>
                <span style={{ color: colourOf(vote.agentId) }}>{vote.agentName}</span>{" "}
                <span className="choice">{vote.choice}</span>
                {vote.rationale && <div className="why">{vote.rationale}</div>}
              </li>
            ))}
          </ul>
        </section>
      )}

      <div ref={endRef} />
    </div>
  );
}

/**
 * Groups consecutive agent turns of the same round into one comparison block.
 *
 * Only meaningful for the parallel policy, where those turns are independent
 * answers to the same prompt and are worth reading against each other.
 */
function groupTurns(messages: Message[], compare: boolean): Block[] {
  if (!compare) {
    return messages.map((message) => ({ kind: "single", message }));
  }

  const blocks: Block[] = [];
  for (const message of messages) {
    const previous = blocks[blocks.length - 1];
    const sameRound =
      message.agent_id !== null &&
      previous?.kind === "compare" &&
      previous.messages[0]?.round === message.round;

    if (sameRound && previous.kind === "compare") {
      previous.messages.push(message);
    } else if (message.agent_id !== null) {
      blocks.push({ kind: "compare", messages: [message] });
    } else {
      blocks.push({ kind: "single", message });
    }
  }
  return blocks;
}

function filterMessages(messages: Message[], query: string): Message[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return messages;
  }
  return messages.filter(
    (message) =>
      message.content.toLowerCase().includes(needle) || message.speaker.toLowerCase().includes(needle),
  );
}

/** Wraps every occurrence of the query so the reader can find it in a long turn. */
function highlight(content: string, query: string): React.ReactNode {
  const needle = query.trim();
  if (!needle) {
    return content;
  }
  const parts: React.ReactNode[] = [];
  const lowered = content.toLowerCase();
  const target = needle.toLowerCase();
  let cursor = 0;

  for (;;) {
    const hit = lowered.indexOf(target, cursor);
    if (hit === -1) {
      parts.push(content.slice(cursor));
      return parts;
    }
    parts.push(content.slice(cursor, hit));
    parts.push(
      <mark key={hit} className="hit">
        {content.slice(hit, hit + needle.length)}
      </mark>,
    );
    cursor = hit + needle.length;
  }
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
