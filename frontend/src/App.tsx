import { useCallback, useEffect, useState } from "react";
import { api, storeToken, storedToken, UnauthorizedError, type RoomDetail, type RoomInput } from "./api";
import { detectLanguage, storeLanguage, STRINGS, type Language, type Strings } from "./i18n";
import { useSession } from "./useSession";
import { Logo } from "./components/Logo";
import { RoomDialog } from "./components/RoomDialog";
import { Transcript } from "./components/Transcript";
import type { PolicyInfo, Provider, Room, RoomSummary } from "./types";

export function App(): React.ReactElement {
  const [language, setLanguage] = useState<Language>(detectLanguage);
  const strings = STRINGS[language];

  const [locked, setLocked] = useState(false);
  const [rooms, setRooms] = useState<RoomSummary[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [policies, setPolicies] = useState<PolicyInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ room: Room | null } | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const [query, setQuery] = useState("");
  const [detail, setDetail] = useState<RoomDetail | null>(null);

  const session = useSession(locked ? null : activeId);

  // The room is fetched over REST as well as over the socket. The socket is the
  // live source once it is ready, but until then the REST copy lets the room
  // render and stay readable even if the socket never connects at all.
  const room = session.room ?? detail;
  const messages = session.room ? session.messages : (detail?.messages ?? []);

  const refresh = useCallback(async () => {
    try {
      const [roomList, providerList, policyList] = await Promise.all([api.rooms(), api.providers(), api.policies()]);
      setRooms(roomList);
      setProviders(providerList);
      setPolicies(policyList);
      setLocked(false);
      setLoadError(null);
    } catch (error) {
      if (error instanceof UnauthorizedError) {
        setLocked(true);
        return;
      }
      setLoadError((error as Error).message);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!activeId && rooms.length > 0) {
      setActiveId(rooms[0]?.id ?? null);
    }
  }, [rooms, activeId]);

  useEffect(() => {
    let active = true;
    setDetail(null);
    if (!activeId || locked) {
      return;
    }
    api
      .room(activeId)
      .then((loaded) => {
        if (active) {
          setDetail(loaded);
        }
      })
      .catch(() => {
        // The socket reports its own failure; a second banner adds nothing.
      });
    return () => {
      active = false;
    };
  }, [activeId, locked]);

  const saveRoom = async (input: RoomInput): Promise<void> => {
    const target = editing?.room;
    const saved = target ? await api.updateRoom(target.id, input) : await api.createRoom(input);
    setEditing(null);
    await refresh();
    setActiveId(saved.id);
  };

  const duplicateRoom = async (): Promise<void> => {
    if (!activeId) {
      return;
    }
    const copy = await api.duplicateRoom(activeId);
    await refresh();
    setActiveId(copy.id);
  };

  const removeRoom = async (): Promise<void> => {
    if (!activeId || !window.confirm(strings.confirmDeleteRoom)) {
      return;
    }
    await api.deleteRoom(activeId);
    setActiveId(null);
    await refresh();
  };

  const clearTranscript = async (): Promise<void> => {
    if (!activeId || !window.confirm(strings.confirmClear)) {
      return;
    }
    await api.clearTranscript(activeId);
    setActiveId(null);
    await refresh();
    setActiveId(activeId);
  };

  const exportTranscript = async (): Promise<void> => {
    if (!activeId) {
      return;
    }
    const markdown = await api.transcript(activeId);
    // Opened in a tab rather than downloaded: a sandboxed page cannot start a
    // download, and a tab lets the user copy or save it themselves.
    const preview = window.open("", "_blank");
    preview?.document.write(`<pre style="white-space:pre-wrap;font:14px ui-monospace,monospace">${escapeHtml(markdown)}</pre>`);
    preview?.document.close();
  };

  const submit = (): void => {
    if (!session.running && prompt.trim()) {
      session.send(prompt);
      setPrompt("");
    }
  };

  if (locked) {
    return <TokenGate strings={strings} onUnlock={() => void refresh()} />;
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Logo />
          <div>
            <h1>HiveMind</h1>
            <small>CHAT</small>
          </div>
        </div>

        <div className="sidebar-section">
          <h2>{strings.rooms}</h2>
          <button type="button" className="ghost" onClick={() => setEditing({ room: null })}>
            +
          </button>
        </div>

        <div className="room-list">
          {rooms.length === 0 && <p className="hint" style={{ padding: "0 4px" }}>{strings.noRooms}</p>}
          {rooms.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className={entry.id === activeId ? "room-item active" : "room-item"}
              onClick={() => {
                setActiveId(entry.id);
                setQuery("");
              }}
            >
              <strong>{entry.name}</strong>
              <span>
                {entry.policy} · {entry.agents} · {entry.messages}
              </span>
            </button>
          ))}
        </div>

        <div className="sidebar-footer">
          <span>{strings.language}</span>
          <select
            value={language}
            aria-label={strings.language}
            onChange={(event) => {
              const next = event.target.value as Language;
              setLanguage(next);
              storeLanguage(next);
            }}
          >
            <option value="en">English</option>
            <option value="de">Deutsch</option>
          </select>
        </div>
      </aside>

      <main className="main">
        {loadError && <div className="banner">{loadError}</div>}

        {!room && (
          <div className="empty">
            <Logo size={54} />
            <p>{rooms.length === 0 ? strings.noRooms : strings.connecting}</p>
          </div>
        )}

        {room && (
          <>
            <div className="room-header">
              <div>
                <h2>{room.name}</h2>
                <div className="meta">
                  {room.policy} · {strings.rounds}: {room.rounds} ·{" "}
                  {room.context_limit === 0
                    ? strings.contextWhole
                    : strings.contextRecent.replace("{n}", String(room.context_limit))}
                  {room.topic ? ` · ${room.topic}` : ""}
                </div>
              </div>
              <div className="actions">
                <input
                  className="search"
                  type="search"
                  value={query}
                  placeholder={strings.search}
                  aria-label={strings.search}
                  onChange={(event) => setQuery(event.target.value)}
                />
                <button type="button" className="ghost" onClick={() => setEditing({ room })}>
                  {strings.agents}
                </button>
                <button type="button" className="ghost" onClick={() => void duplicateRoom()}>
                  {strings.duplicateRoom}
                </button>
                <button type="button" className="ghost" onClick={() => void exportTranscript()}>
                  {strings.exportTranscript}
                </button>
                <button type="button" className="ghost" onClick={() => void clearTranscript()}>
                  {strings.clearTranscript}
                </button>
                <button type="button" className="ghost danger" onClick={() => void removeRoom()}>
                  {strings.deleteRoom}
                </button>
              </div>
            </div>

            <div className="agent-chips">
              {room.agents.map((agent) => (
                <span key={agent.id} className={agent.enabled ? "chip" : "chip disabled"}>
                  <span className="dot" style={{ background: agent.colour }} />
                  {agent.name}
                  <span className="model">{agent.model}</span>
                </span>
              ))}
              {room.agents.length === 0 && <span className="hint">{strings.noAgents}</span>}
            </div>

            <Transcript
              strings={strings}
              messages={messages}
              pending={session.pending}
              votes={session.votes}
              agents={room.agents}
              round={session.round}
              compare={room.policy === "parallel"}
              query={query}
            />

            {session.error && <div className="banner">{session.error}</div>}

            <div className="composer">
              <div style={{ flex: 1 }}>
                <div className="status">
                  {session.running ? strings.running : session.status === "open" ? "" : strings[session.status === "connecting" ? "connecting" : "disconnected"]}
                </div>
                <textarea
                  value={prompt}
                  placeholder={strings.promptPlaceholder}
                  onChange={(event) => setPrompt(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                      event.preventDefault();
                      submit();
                    }
                  }}
                  disabled={session.status !== "open" || room.agents.length === 0}
                />
              </div>
              {session.running ? (
                <button type="button" onClick={session.stop}>
                  {strings.stop}
                </button>
              ) : (
                <button
                  type="button"
                  className="primary"
                  onClick={submit}
                  disabled={session.status !== "open" || !prompt.trim() || room.agents.length === 0}
                >
                  {strings.send}
                </button>
              )}
            </div>
          </>
        )}
      </main>

      {editing && (
        <RoomDialog
          strings={strings}
          providers={providers}
          policies={policies}
          room={editing.room}
          onSave={(input) => void saveRoom(input)}
          onCancel={() => setEditing(null)}
        />
      )}
    </div>
  );
}

function TokenGate({ strings, onUnlock }: { strings: Strings; onUnlock: () => void }): React.ReactElement {
  const [value, setValue] = useState(storedToken());

  return (
    <div className="overlay">
      <div className="dialog" style={{ width: "min(440px, 100%)" }}>
        <h2>{strings.tokenTitle}</h2>
        <p className="hint" style={{ marginBottom: 16 }}>
          {strings.tokenHint}
        </p>
        <div className="field">
          <input
            type="password"
            value={value}
            placeholder={strings.tokenPlaceholder}
            autoFocus
            onChange={(event) => setValue(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                storeToken(value.trim());
                onUnlock();
              }
            }}
          />
        </div>
        <div className="dialog-actions">
          <button
            type="button"
            className="primary"
            onClick={() => {
              storeToken(value.trim());
              onUnlock();
            }}
          >
            {strings.unlock}
          </button>
        </div>
      </div>
    </div>
  );
}

function escapeHtml(raw: string): string {
  return raw.replace(/[&<>]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[character] ?? character);
}
