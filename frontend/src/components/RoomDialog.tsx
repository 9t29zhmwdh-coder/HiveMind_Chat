import { useState } from "react";
import type { AgentInput, RoomInput } from "../api";
import type { Strings } from "../i18n";
import type { PolicyInfo, Provider, Room, TurnPolicy } from "../types";
import { AgentDialog } from "./AgentDialog";

interface Props {
  strings: Strings;
  providers: Provider[];
  policies: PolicyInfo[];
  room: Room | null;
  onSave: (input: RoomInput) => void;
  onCancel: () => void;
}

const PALETTE = ["#e8b339", "#4f9dde", "#59c08a", "#d97757", "#b07be0", "#5bc0be"];

/** Creates or edits a room together with its agent list. */
export function RoomDialog({ strings, providers, policies, room, onSave, onCancel }: Props): React.ReactElement {
  const [name, setName] = useState(room?.name ?? "");
  const [topic, setTopic] = useState(room?.topic ?? "");
  const [policy, setPolicy] = useState<TurnPolicy>(room?.policy ?? "round_robin");
  const [rounds, setRounds] = useState(room?.rounds ?? 1);
  const [agents, setAgents] = useState<AgentInput[]>(room?.agents.map(toInput) ?? []);
  const [moderatorId, setModeratorId] = useState<string | null>(room?.moderator_id ?? null);
  const [editing, setEditing] = useState<{ agent: AgentInput; index: number } | null>(null);

  const summary = policies.find((entry) => entry.id === policy)?.summary ?? "";
  const canSave = name.trim().length > 0 && (policy !== "moderated" || moderatorId !== null);

  const upsertAgent = (agent: AgentInput, index: number): void => {
    setAgents((current) => {
      const next = [...current];
      if (index < 0) {
        next.push(agent);
      } else {
        next[index] = agent;
      }
      return next;
    });
    setEditing(null);
  };

  const removeAgent = (index: number): void => {
    setAgents((current) => current.filter((_, position) => position !== index));
  };

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="dialog">
        <h2>{room ? room.name : strings.newRoom}</h2>

        <div className="grid-2">
          <div className="field">
            <label htmlFor="room-name">{strings.name}</label>
            <input id="room-name" value={name} onChange={(event) => setName(event.target.value)} autoFocus />
          </div>
          <div className="field">
            <label htmlFor="room-topic">{strings.topic}</label>
            <input id="room-topic" value={topic} onChange={(event) => setTopic(event.target.value)} />
          </div>
        </div>

        <div className="grid-2">
          <div className="field">
            <label htmlFor="room-policy">{strings.policy}</label>
            <select id="room-policy" value={policy} onChange={(event) => setPolicy(event.target.value as TurnPolicy)}>
              {policies.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.id}
                </option>
              ))}
            </select>
            <p className="hint">{summary}</p>
          </div>
          <div className="field">
            <label htmlFor="room-rounds">{strings.rounds}</label>
            <input
              id="room-rounds"
              type="number"
              min={1}
              max={20}
              value={rounds}
              onChange={(event) => setRounds(Number(event.target.value))}
            />
          </div>
        </div>

        {policy === "moderated" && (
          <div className="field">
            <label htmlFor="room-moderator">Moderator</label>
            <select
              id="room-moderator"
              value={moderatorId ?? ""}
              onChange={(event) => setModeratorId(event.target.value || null)}
            >
              <option value="">(none)</option>
              {agents.map((agent) => (
                <option key={agent.id ?? agent.name} value={agent.id ?? ""}>
                  {agent.name}
                </option>
              ))}
            </select>
            {moderatorId === null && <p className="warn">A moderated room needs a moderator.</p>}
          </div>
        )}

        <div className="field">
          <label>{strings.agents}</label>
          {agents.map((agent, index) => (
            <div className="agent-row" key={agent.id ?? `${agent.name}-${index}`}>
              <span className="dot" style={{ background: agent.colour, width: 10, height: 10, borderRadius: "50%" }} />
              <div className="grow">
                <div>{agent.name}</div>
                <div className="sub">
                  {agent.provider_id} · {agent.model}
                  {agent.reasoning ? " · reasoning" : ""}
                  {agent.enabled ? "" : " · off"}
                </div>
              </div>
              <button type="button" className="ghost" onClick={() => setEditing({ agent, index })}>
                {strings.editAgent}
              </button>
              <button type="button" className="ghost danger" onClick={() => removeAgent(index)}>
                {strings.remove}
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() => setEditing({ agent: blankAgent(providers, agents.length), index: -1 })}
            disabled={providers.length === 0}
          >
            {strings.addAgent}
          </button>
        </div>

        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={onCancel}>
            {strings.cancel}
          </button>
          <button
            type="button"
            className="primary"
            disabled={!canSave}
            onClick={() =>
              onSave({
                name: name.trim(),
                topic: topic.trim(),
                policy,
                rounds,
                moderator_id: policy === "moderated" ? moderatorId : null,
                agents,
              })
            }
          >
            {strings.save}
          </button>
        </div>
      </div>

      {editing && (
        <AgentDialog
          strings={strings}
          providers={providers}
          agent={editing.agent}
          onSave={(agent) => upsertAgent(agent, editing.index)}
          onCancel={() => setEditing(null)}
        />
      )}
    </div>
  );
}

function toInput(agent: Room["agents"][number]): AgentInput {
  return {
    id: agent.id,
    name: agent.name,
    provider_id: agent.provider_id,
    model: agent.model,
    persona: agent.persona,
    temperature: agent.temperature,
    max_tokens: agent.max_tokens,
    colour: agent.colour,
    reasoning: agent.reasoning,
    enabled: agent.enabled,
  };
}

function blankAgent(providers: Provider[], index: number): AgentInput {
  return {
    name: "",
    provider_id: providers[0]?.id ?? "",
    model: "",
    persona: "",
    temperature: 0.7,
    max_tokens: 1024,
    colour: PALETTE[index % PALETTE.length] ?? "#e8e8e8",
    reasoning: false,
    enabled: true,
  };
}
