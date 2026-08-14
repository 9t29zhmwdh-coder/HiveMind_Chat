import { useEffect, useState } from "react";
import { api, type AgentInput } from "../api";
import type { Strings } from "../i18n";
import type { Provider } from "../types";

interface Props {
  strings: Strings;
  providers: Provider[];
  agent: AgentInput;
  onSave: (agent: AgentInput) => void;
  onCancel: () => void;
}

export function AgentDialog({ strings, providers, agent, onSave, onCancel }: Props): React.ReactElement {
  const [draft, setDraft] = useState<AgentInput>(agent);
  const [models, setModels] = useState<string[]>([]);
  const [modelsError, setModelsError] = useState<string | null>(null);

  const provider = providers.find((entry) => entry.id === draft.provider_id);

  // The model list comes from the provider itself, so a typo in a model name
  // shows up here rather than as a failed turn later.
  useEffect(() => {
    let active = true;
    setModels([]);
    setModelsError(null);
    if (!draft.provider_id) {
      return;
    }
    api
      .models(draft.provider_id)
      .then((list) => {
        if (active) {
          setModels(list);
        }
      })
      .catch((error: Error) => {
        if (active) {
          setModelsError(error.message);
        }
      });
    return () => {
      active = false;
    };
  }, [draft.provider_id]);

  const update = <K extends keyof AgentInput>(key: K, value: AgentInput[K]): void =>
    setDraft((current) => ({ ...current, [key]: value }));

  const canSave = draft.name.trim().length > 0 && draft.model.trim().length > 0 && draft.provider_id.length > 0;

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="dialog">
        <h2>{agent.id ? strings.editAgent : strings.addAgent}</h2>

        <div className="grid-3">
          <div className="field">
            <label htmlFor="agent-name">{strings.name}</label>
            <input id="agent-name" value={draft.name} onChange={(event) => update("name", event.target.value)} autoFocus />
          </div>
          <div className="field">
            <label htmlFor="agent-provider">{strings.provider}</label>
            <select
              id="agent-provider"
              value={draft.provider_id}
              onChange={(event) => {
                update("provider_id", event.target.value);
                update("model", "");
              }}
            >
              {providers.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.label}
                  {entry.local ? ` (${strings.localProvider})` : ""}
                </option>
              ))}
            </select>
            {provider && !provider.credential_available && (
              <p className="warn">
                {strings.credentialMissing}: {provider.credential_env}
              </p>
            )}
          </div>
          <div className="field">
            <label htmlFor="agent-model">{strings.model}</label>
            <input
              id="agent-model"
              list="agent-model-options"
              value={draft.model}
              onChange={(event) => update("model", event.target.value)}
            />
            <datalist id="agent-model-options">
              {models.map((model) => (
                <option key={model} value={model} />
              ))}
            </datalist>
            {modelsError && <p className="hint">{modelsError}</p>}
          </div>
        </div>

        <div className="field">
          <label htmlFor="agent-persona">{strings.persona}</label>
          <textarea
            id="agent-persona"
            value={draft.persona}
            onChange={(event) => update("persona", event.target.value)}
          />
          <p className="hint">{strings.personaHint}</p>
        </div>

        <div className="grid-3">
          <div className="field">
            <label htmlFor="agent-temperature">{strings.temperature}</label>
            <input
              id="agent-temperature"
              type="number"
              step={0.1}
              min={0}
              max={2}
              value={draft.temperature}
              onChange={(event) => update("temperature", Number(event.target.value))}
            />
          </div>
          <div className="field">
            <label htmlFor="agent-tokens">{strings.maxTokens}</label>
            <input
              id="agent-tokens"
              type="number"
              min={1}
              max={32000}
              value={draft.max_tokens}
              onChange={(event) => update("max_tokens", Number(event.target.value))}
            />
          </div>
          <div className="field">
            <label htmlFor="agent-colour">{strings.colour}</label>
            <input
              id="agent-colour"
              type="color"
              value={draft.colour}
              onChange={(event) => update("colour", event.target.value)}
            />
          </div>
        </div>

        <div className="checkbox">
          <input
            id="agent-reasoning"
            type="checkbox"
            checked={draft.reasoning}
            onChange={(event) => update("reasoning", event.target.checked)}
          />
          <div>
            <label htmlFor="agent-reasoning">{strings.reasoning}</label>
            <p className="hint">{strings.reasoningHint}</p>
          </div>
        </div>

        <div className="checkbox">
          <input
            id="agent-enabled"
            type="checkbox"
            checked={draft.enabled}
            onChange={(event) => update("enabled", event.target.checked)}
          />
          <label htmlFor="agent-enabled">{strings.enabled}</label>
        </div>

        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={onCancel}>
            {strings.cancel}
          </button>
          <button type="button" className="primary" disabled={!canSave} onClick={() => onSave(draft)}>
            {strings.save}
          </button>
        </div>
      </div>
    </div>
  );
}
