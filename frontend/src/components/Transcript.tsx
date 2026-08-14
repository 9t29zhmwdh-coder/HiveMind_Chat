import { useEffect, useRef } from "react";
import type { Strings } from "../i18n";
import type { Agent, Message, PendingTurn, Vote } from "../types";

interface Props {
  strings: Strings;
  messages: Message[];
  pending: PendingTurn[];
  votes: Vote[];
  agents: Agent[];
  round: { current: number; total: number } | null;
}

export function Transcript({ strings, messages, pending, votes, agents, round }: Props): React.ReactElement {
  const endRef = useRef<HTMLDivElement>(null);
  const streamedLength = pending.reduce((total, turn) => total + turn.text.length, 0);

  // Follow the conversation as it grows, including while a turn streams.
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length, streamedLength, votes.length]);

  const colourOf = (agentId: string | null): string =>
    agents.find((agent) => agent.id === agentId)?.colour ?? "#6c6c78";

  return (
    <div className="transcript">
      {messages.map((message) => (
        <article key={message.id} className={message.agent_id ? "turn" : "turn user"} style={borderColour(message, colourOf)}>
          <header>
            <span className="who" style={{ color: message.agent_id ? colourOf(message.agent_id) : undefined }}>
              {message.agent_id ? message.speaker : strings.you}
            </span>
            <span className="when">{formatTime(message.created_at)}</span>
          </header>
          <div className="body">{message.content}</div>
        </article>
      ))}

      {round && (
        <div className="round-divider">
          {strings.round} {round.current}/{round.total}
        </div>
      )}

      {pending.map((turn) => (
        <article key={turn.agentId} className="turn streaming" style={{ borderLeftColor: turn.colour }}>
          <header>
            <span className="who" style={{ color: turn.colour }}>
              {turn.agentName}
            </span>
          </header>
          <div className="body">{turn.text}</div>
        </article>
      ))}

      {votes.length > 0 && (
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

function borderColour(message: Message, colourOf: (id: string | null) => string): React.CSSProperties {
  return message.agent_id ? { borderLeftColor: colourOf(message.agent_id) } : {};
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
